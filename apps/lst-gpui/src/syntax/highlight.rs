use super::{
    catalog::{self, GrammarId},
    SyntaxLanguage, SyntaxSpan,
};
use crate::{diagnostics, ui::theme::SyntaxRole};
use lst_editor::{BufferDelta, BufferEdit};
use ropey::Rope;
use std::{cell::RefCell, ops::Range, rc::Rc, time::Instant};
use tree_sitter::{InputEdit, Node, Parser, Point, QueryCursor, StreamingIterator, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StructuralPair {
    pub(crate) open: usize,
    pub(crate) close: usize,
    pub(crate) depth: u16,
    pub(crate) parent: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StructuralToken {
    pub(crate) at: usize,
    pub(crate) depth: u16,
    pub(crate) matched: bool,
    pub(crate) pair: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StructuralSnapshot {
    pub(crate) revision: u64,
    pub(crate) pairs: Vec<StructuralPair>,
    pub(crate) tokens: Vec<StructuralToken>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SyntaxInvalidation {
    Full,
    Lines(Range<usize>),
}

impl SyntaxInvalidation {
    pub(crate) fn line_range(&self, line_count: usize) -> Range<usize> {
        match self {
            Self::Full => 0..line_count,
            Self::Lines(lines) => lines.start.min(line_count)..lines.end.min(line_count),
        }
    }

    pub(crate) fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub(crate) fn from_buffer_delta(
        new_buffer: &Rope,
        delta: &BufferDelta,
        previous_line_count: Option<usize>,
    ) -> Self {
        match delta {
            BufferDelta::Unchanged => Self::Lines(0..0),
            BufferDelta::FullReplace => Self::Full,
            BufferDelta::Edits(edits) if previous_line_count == Some(new_buffer.len_lines()) => {
                let changed = edited_line_range(new_buffer, edits).unwrap_or(0..new_buffer.len_lines());
                Self::Lines(changed.start.saturating_sub(1)..changed.end.saturating_add(1).min(new_buffer.len_lines()))
            }
            BufferDelta::Edits(_) => Self::Full,
        }
    }
}

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
    structure: Rc<RefCell<StructuralSnapshot>>,
    structure_remapped_last_update: bool,
}

impl TabSyntaxState {
    pub(crate) fn parse_initial(language: SyntaxLanguage, buffer: &Rope, revision: u64) -> Option<Self> {
        let grammar = catalog::grammar(catalog::root_grammar(language));
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        let parse_started = diagnostics::trace_enabled().then(Instant::now);
        let tree = parse_rope(&mut parser, buffer, None)?;
        if let Some(started) = parse_started {
            diagnostics::record_ms("syntax_parse_initial_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        let structure_started = diagnostics::trace_enabled().then(Instant::now);
        let structure = structural_snapshot(language, &tree, buffer, revision);
        if let Some(started) = structure_started {
            diagnostics::record_ms("syntax_structure_initial_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        Some(Self {
            language,
            revision,
            parser,
            tree,
            parsed_buffer: buffer.clone(),
            structure: Rc::new(RefCell::new(structure)),
            structure_remapped_last_update: false,
        })
    }

    /// Update the tree to match `new_buffer` and return the smallest safe
    /// line window whose cached highlight spans need replacing. Changes that
    /// alter line topology intentionally fall back to `Full`; ordinary typing
    /// keeps all unaffected per-line spans.
    pub(crate) fn update(&mut self, new_buffer: &Rope, delta: BufferDelta, new_revision: u64) -> SyntaxInvalidation {
        let parse_started = diagnostics::trace_enabled().then(Instant::now);
        let mut changed_byte_ranges = Vec::new();
        let mut parse_succeeded = true;
        let root_config = catalog::grammar(catalog::root_grammar(self.language));
        let edits_touch_old_injection = match &delta {
            BufferDelta::Edits(edits) => edits_touch_injected_syntax(
                &self.tree,
                &self.parsed_buffer,
                root_config,
                edits.iter().map(|edit| edit.range.clone()),
            ),
            BufferDelta::Unchanged | BufferDelta::FullReplace => false,
        };
        let invalidation = match &delta {
            BufferDelta::Unchanged => {
                // A changed revision without a delta means a consumer missed
                // an edit batch. Reparse and rebuild rather than guessing.
                if let Some(tree) = parse_rope(&mut self.parser, new_buffer, None) {
                    self.tree = tree;
                } else {
                    parse_succeeded = false;
                }
                SyntaxInvalidation::Full
            }
            BufferDelta::FullReplace => {
                if let Some(tree) = parse_rope(&mut self.parser, new_buffer, None) {
                    self.tree = tree;
                } else {
                    parse_succeeded = false;
                }
                SyntaxInvalidation::Full
            }
            BufferDelta::Edits(edits) => {
                let line_topology_changed = self.parsed_buffer.len_lines() != new_buffer.len_lines();
                let mut changed_lines = edited_line_range(new_buffer, edits);
                // Apply edits in reverse order so each edit's pre-batch
                // coordinates (in `self.parsed_buffer`) stay valid — later
                // edits don't shift earlier positions.
                for edit in edits.iter().rev() {
                    let input_edit = input_edit_for(&self.parsed_buffer, edit);
                    self.tree.edit(&input_edit);
                }
                if let Some(tree) = parse_rope(&mut self.parser, new_buffer, Some(&self.tree)) {
                    for range in self.tree.changed_ranges(&tree) {
                        changed_byte_ranges.push(range.start_byte..range.end_byte);
                        include_line_range(
                            &mut changed_lines,
                            range.start_point.row..range.end_point.row.saturating_add(1),
                        );
                    }
                    self.tree = tree;
                } else {
                    parse_succeeded = false;
                }
                if line_topology_changed {
                    SyntaxInvalidation::Full
                } else {
                    let line_count = new_buffer.len_lines();
                    let changed_lines = changed_lines.unwrap_or(0..line_count);
                    SyntaxInvalidation::Lines(
                        changed_lines.start.saturating_sub(1)..changed_lines.end.saturating_add(1).min(line_count),
                    )
                }
            }
        };
        if let Some(started) = parse_started {
            diagnostics::record_ms("syntax_parse_update_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        let structure_started = diagnostics::trace_enabled().then(Instant::now);
        let edits_touch_new_injection = match &delta {
            BufferDelta::Edits(edits) => {
                edits_touch_injected_syntax(&self.tree, new_buffer, root_config, post_edit_ranges(edits))
            }
            BufferDelta::Unchanged | BufferDelta::FullReplace => false,
        };
        let structure_remapped = parse_succeeded
            && !edits_touch_old_injection
            && !edits_touch_new_injection
            && matches!(&delta, BufferDelta::Edits(edits) if can_remap_structural_snapshot(
                &self.parsed_buffer,
                new_buffer,
                edits,
                &changed_byte_ranges,
            ));
        if structure_remapped {
            let BufferDelta::Edits(edits) = &delta else {
                unreachable!("structure remapping only applies to edit deltas");
            };
            remap_structural_snapshot(&mut self.structure.borrow_mut(), edits, new_revision);
        } else {
            *self.structure.borrow_mut() = structural_snapshot(self.language, &self.tree, new_buffer, new_revision);
        }
        if let Some(started) = structure_started {
            diagnostics::record_ms("syntax_structure_update_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        self.parsed_buffer = new_buffer.clone();
        self.revision = new_revision;
        self.structure_remapped_last_update = structure_remapped;
        invalidation
    }

    #[cfg(test)]
    pub(crate) fn structure(&self) -> std::cell::Ref<'_, StructuralSnapshot> {
        self.structure.borrow()
    }

    pub(crate) fn shared_structure(&self) -> Rc<RefCell<StructuralSnapshot>> {
        self.structure.clone()
    }

    pub(crate) fn structure_was_remapped(&self) -> bool {
        self.structure_remapped_last_update
    }

    /// Returns parser-owned selection ranges as plain character offsets.
    /// Callers never observe tree-sitter nodes or byte coordinates.
    pub(crate) fn selection_ranges_at(&self, char_offsets: &[usize]) -> Vec<Range<usize>> {
        let mut byte_ranges = Vec::new();
        let root_grammar = catalog::root_grammar(self.language);
        let root_config = catalog::grammar(root_grammar);
        let injections = collect_rope_injection_matches(
            &self.tree,
            &self.parsed_buffer,
            root_config,
            0..self.parsed_buffer.len_bytes(),
        );
        for &char_offset in char_offsets {
            let byte = self
                .parsed_buffer
                .char_to_byte(char_offset.min(self.parsed_buffer.len_chars()));
            let root = self.tree.root_node();
            let Some(mut node) = root.descendant_for_byte_range(byte, byte.saturating_add(1)) else {
                continue;
            };
            loop {
                if node.start_byte() < node.end_byte() {
                    byte_ranges.push(node.byte_range());
                }
                let Some(parent) = node.parent() else { break };
                node = parent;
            }
            for injection in injections
                .iter()
                .filter(|injection| injection.content_start <= byte && byte <= injection.content_end)
            {
                let content = injection.content_start..injection.content_end;
                let source = self.parsed_buffer.byte_slice(content.clone()).to_string();
                let Some(tree) = parse_sub_source(injection.embedded, source.as_bytes()) else {
                    continue;
                };
                collect_injected_selection_ranges(
                    injection.embedded,
                    &tree,
                    source.as_bytes(),
                    injection.content_start,
                    byte.saturating_sub(injection.content_start),
                    &mut byte_ranges,
                );
            }
        }
        let mut ranges: Vec<Range<usize>> = byte_ranges
            .into_iter()
            .filter(|range| range.start < range.end && range.end <= self.parsed_buffer.len_bytes())
            .map(|range| self.parsed_buffer.byte_to_char(range.start)..self.parsed_buffer.byte_to_char(range.end))
            .collect();
        ranges.sort_by_key(|range| (range.end.saturating_sub(range.start), range.start, range.end));
        ranges.dedup();
        ranges
    }

    #[cfg(test)]
    pub(crate) fn compute_spans(&self) -> (Vec<Vec<SyntaxSpan>>, Vec<u32>) {
        self.compute_spans_for_lines(0..self.parsed_buffer.len_lines())
    }

    pub(crate) fn compute_spans_for_lines(&self, lines: Range<usize>) -> (Vec<Vec<SyntaxSpan>>, Vec<u32>) {
        let line_count = self.parsed_buffer.len_lines();
        let lines = lines.start.min(line_count)..lines.end.min(line_count);
        if lines.is_empty() {
            return (Vec::new(), Vec::new());
        }
        // Line topology must match `EditorTab::lines()` (which iterates
        // `Rope::lines()` and trims trailing \n/\r), otherwise the byte-
        // length guard in viewport disables the cache for the wrong line
        // indices on files containing lone CR or other Unicode separators
        // that ropey treats as line breaks.
        let (line_starts, display_ends) = line_bounds_from_rope(&self.parsed_buffer, lines.clone());
        let line_byte_lens: Vec<u32> = line_starts
            .iter()
            .zip(display_ends.iter())
            .map(|(start, end)| (end.saturating_sub(*start)) as u32)
            .collect();
        let mut spans = vec![Vec::new(); line_starts.len()];
        let byte_range = self.parsed_buffer.line_to_byte(lines.start)..if lines.end < line_count {
            self.parsed_buffer.line_to_byte(lines.end)
        } else {
            self.parsed_buffer.len_bytes()
        };

        let root_grammar = catalog::root_grammar(self.language);
        let mut captures = Vec::new();
        collect_rope_captures(
            root_grammar,
            &self.tree,
            &self.parsed_buffer,
            byte_range.clone(),
            0,
            &mut captures,
        );

        emit_non_overlapping_spans(&captures, &mut spans, &line_starts, &display_ends, byte_range);
        (spans, line_byte_lens)
    }
}

fn edits_touch_injected_syntax(
    tree: &Tree,
    buffer: &Rope,
    config: &catalog::GrammarConfig,
    ranges: impl IntoIterator<Item = Range<usize>>,
) -> bool {
    if config.injections.is_none() || buffer.len_chars() == 0 {
        return false;
    }
    let len_chars = buffer.len_chars();
    ranges.into_iter().any(|range| {
        // Include one character of context on both sides so edits to an
        // injection boundary (a fence, tag, or template delimiter) query the
        // containing injection pattern as well as edits within its content.
        let start = range.start.min(len_chars);
        let end = range.end.min(len_chars).max(start);
        let context_start = start.saturating_sub(1);
        let context_end = end.max(start.saturating_add(1)).saturating_add(1).min(len_chars);
        let bytes = buffer.char_to_byte(context_start)..buffer.char_to_byte(context_end);
        !collect_rope_injection_matches(tree, buffer, config, bytes).is_empty()
    })
}

fn post_edit_ranges(edits: &[BufferEdit]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut shift = 0isize;
    edits.iter().map(move |edit| {
        let start = edit.range.start.saturating_add_signed(shift);
        let inserted = edit.replacement.chars().count();
        let removed = edit.range.end.saturating_sub(edit.range.start);
        shift = shift.saturating_add(
            isize::try_from(inserted).unwrap_or(isize::MAX) - isize::try_from(removed).unwrap_or(isize::MAX),
        );
        start..start.saturating_add(inserted)
    })
}

fn can_remap_structural_snapshot(
    old_buffer: &Rope,
    new_buffer: &Rope,
    edits: &[BufferEdit],
    changed_byte_ranges: &[Range<usize>],
) -> bool {
    for edit in edits {
        let removed = edit.range.start.min(old_buffer.len_chars())..edit.range.end.min(old_buffer.len_chars());
        if old_buffer.slice(removed).chars().any(is_structural_candidate)
            || edit.replacement.chars().any(is_structural_candidate)
        {
            return false;
        }
    }
    changed_byte_ranges.iter().all(|range| {
        let start = range.start.min(new_buffer.len_bytes());
        let end = range.end.min(new_buffer.len_bytes()).max(start);
        !new_buffer.byte_slice(start..end).chars().any(is_structural_candidate)
    })
}

fn is_structural_candidate(ch: char) -> bool {
    matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>')
}

fn remap_structural_snapshot(snapshot: &mut StructuralSnapshot, edits: &[BufferEdit], revision: u64) {
    let offsets = OffsetMapper::new(edits);
    for pair in &mut snapshot.pairs {
        pair.open = offsets.map(pair.open);
        pair.close = offsets.map(pair.close);
    }
    for token in &mut snapshot.tokens {
        token.at = offsets.map(token.at);
    }
    snapshot.revision = revision;
}

/// Maps pre-edit character offsets through a normalized, sorted edit batch.
/// Building prefix shifts once keeps structural remapping O(tokens log edits)
/// for large multi-cursor transactions instead of O(tokens * edits).
struct OffsetMapper<'a> {
    edits: &'a [BufferEdit],
    shifts: Vec<isize>,
}

impl<'a> OffsetMapper<'a> {
    fn new(edits: &'a [BufferEdit]) -> Self {
        let mut shifts = Vec::with_capacity(edits.len() + 1);
        shifts.push(0isize);
        for edit in edits {
            let removed = edit.range.end.saturating_sub(edit.range.start);
            let inserted = edit.replacement.chars().count();
            shifts.push(shifts.last().copied().unwrap_or_default().saturating_add(
                isize::try_from(inserted).unwrap_or(isize::MAX) - isize::try_from(removed).unwrap_or(isize::MAX),
            ));
        }
        Self { edits, shifts }
    }

    fn map(&self, at: usize) -> usize {
        let completed = self.edits.partition_point(|edit| edit.range.end <= at);
        at.saturating_add_signed(self.shifts[completed])
    }
}

fn collect_injected_selection_ranges(
    grammar: GrammarId,
    tree: &Tree,
    source: &[u8],
    byte_offset: usize,
    local_byte: usize,
    out: &mut Vec<Range<usize>>,
) {
    if let Some(mut node) = tree
        .root_node()
        .descendant_for_byte_range(local_byte, local_byte.saturating_add(1))
    {
        loop {
            if node.start_byte() < node.end_byte() {
                out.push(byte_offset + node.start_byte()..byte_offset + node.end_byte());
            }
            let Some(parent) = node.parent() else { break };
            node = parent;
        }
    }
    let config = catalog::grammar(grammar);
    for injection in collect_injection_matches(tree, source, config, 0..source.len()) {
        if !(injection.content_start <= local_byte && local_byte <= injection.content_end) {
            continue;
        }
        let sub_source = &source[injection.content_start..injection.content_end];
        let Some(sub_tree) = parse_sub_source(injection.embedded, sub_source) else {
            continue;
        };
        collect_injected_selection_ranges(
            injection.embedded,
            &sub_tree,
            sub_source,
            byte_offset + injection.content_start,
            local_byte.saturating_sub(injection.content_start),
            out,
        );
    }
}

fn structural_snapshot(language: SyntaxLanguage, tree: &Tree, buffer: &Rope, revision: u64) -> StructuralSnapshot {
    #[derive(Clone, Copy)]
    struct Delimiter {
        byte: usize,
        ch: char,
    }

    fn collect(node: Node<'_>, grammar: GrammarId, byte_offset: usize, out: &mut Vec<Delimiter>) {
        if node.child_count() == 0 {
            let kind = node.kind().as_bytes();
            if kind.len() == 1 {
                let ch = kind[0] as char;
                let basic = matches!(ch, '(' | ')' | '[' | ']' | '{' | '}');
                let angle = matches!(ch, '<' | '>') && angle_is_structural(grammar, node);
                if (basic || angle) && node.end_byte() == node.start_byte() + 1 {
                    out.push(Delimiter {
                        byte: byte_offset + node.start_byte(),
                        ch,
                    });
                }
            } else if angle_is_structural(grammar, node) {
                let angle = match kind {
                    b"</" => Some((node.start_byte(), '<')),
                    b"/>" => Some((node.end_byte().saturating_sub(1), '>')),
                    _ => None,
                };
                if let Some((byte, ch)) = angle {
                    out.push(Delimiter {
                        byte: byte_offset + byte,
                        ch,
                    });
                }
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect(child, grammar, byte_offset, out);
        }
    }

    fn collect_injected(
        grammar: GrammarId,
        tree: &Tree,
        source: &[u8],
        byte_offset: usize,
        streams: &mut Vec<Vec<Delimiter>>,
    ) {
        let config = catalog::grammar(grammar);
        let injections = collect_injection_matches(tree, source, config, 0..source.len());
        let mut delimiters = Vec::new();
        collect(tree.root_node(), grammar, byte_offset, &mut delimiters);
        delimiters.retain(|delimiter| {
            let local_byte = delimiter.byte.saturating_sub(byte_offset);
            !injections
                .iter()
                .any(|injection| injection.content_start <= local_byte && local_byte < injection.content_end)
        });
        streams.push(delimiters);

        for injection in injections {
            if injection.content_end <= injection.content_start {
                continue;
            }
            let sub_source = &source[injection.content_start..injection.content_end];
            let Some(sub_tree) = parse_sub_source(injection.embedded, sub_source) else {
                continue;
            };
            collect_injected(
                injection.embedded,
                &sub_tree,
                sub_source,
                byte_offset + injection.content_start,
                streams,
            );
        }
    }

    fn matching(open: char, close: char) -> bool {
        matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}') | ('<', '>'))
    }

    let root_grammar = catalog::root_grammar(language);
    let root_config = catalog::grammar(root_grammar);
    let root_injections = collect_rope_injection_matches(tree, buffer, root_config, 0..buffer.len_bytes());
    let mut streams = Vec::new();
    let mut root_delimiters = Vec::new();
    collect(tree.root_node(), root_grammar, 0, &mut root_delimiters);
    root_delimiters.retain(|delimiter| {
        !root_injections
            .iter()
            .any(|injection| injection.content_start <= delimiter.byte && delimiter.byte < injection.content_end)
    });
    streams.push(root_delimiters);
    for injection in root_injections {
        if injection.content_end <= injection.content_start {
            continue;
        }
        let source = buffer
            .byte_slice(injection.content_start..injection.content_end)
            .to_string();
        let Some(sub_tree) = parse_sub_source(injection.embedded, source.as_bytes()) else {
            continue;
        };
        collect_injected(
            injection.embedded,
            &sub_tree,
            source.as_bytes(),
            injection.content_start,
            &mut streams,
        );
    }

    let mut pairs = Vec::new();
    let mut unmatched = Vec::new();
    for mut delimiters in streams {
        delimiters.sort_by_key(|delimiter| delimiter.byte);
        delimiters.dedup_by_key(|delimiter| delimiter.byte);
        let mut stack: Vec<Delimiter> = Vec::new();
        for delimiter in delimiters {
            if matches!(delimiter.ch, '(' | '[' | '{' | '<') {
                stack.push(delimiter);
            } else if let Some(open) = stack.last().copied().filter(|open| matching(open.ch, delimiter.ch)) {
                stack.pop();
                pairs.push(StructuralPair {
                    open: buffer.byte_to_char(open.byte),
                    close: buffer.byte_to_char(delimiter.byte),
                    depth: 0,
                    parent: None,
                });
            } else {
                unmatched.push(buffer.byte_to_char(delimiter.byte));
            }
        }
        unmatched.extend(stack.into_iter().map(|delimiter| buffer.byte_to_char(delimiter.byte)));
    }
    pairs.sort_by_key(|pair| pair.open);
    pairs.dedup_by_key(|pair| (pair.open, pair.close));
    assign_pair_parents(&mut pairs);
    unmatched.sort_unstable();
    unmatched.dedup();

    let mut tokens = Vec::with_capacity(pairs.len() * 2 + unmatched.len());
    for (pair_index, pair) in pairs.iter().enumerate() {
        tokens.push(StructuralToken {
            at: pair.open,
            depth: pair.depth,
            matched: true,
            pair: Some(pair_index),
        });
        tokens.push(StructuralToken {
            at: pair.close,
            depth: pair.depth,
            matched: true,
            pair: Some(pair_index),
        });
    }
    tokens.extend(unmatched.into_iter().map(|at| StructuralToken {
        at,
        depth: 0,
        matched: false,
        pair: None,
    }));
    tokens.sort_by_key(|token| token.at);
    StructuralSnapshot {
        revision,
        pairs,
        tokens,
    }
}

fn angle_is_structural(grammar: GrammarId, node: Node<'_>) -> bool {
    if !matches!(grammar, GrammarId::Jsx | GrammarId::Tsx | GrammarId::Html) {
        return false;
    }
    node.parent().is_some_and(|parent| {
        let kind = parent.kind();
        kind.contains("tag") || kind.contains("element") || kind.contains("fragment")
    })
}

pub(crate) fn plain_structural_snapshot(
    buffer: &Rope,
    revision: u64,
    structural_pairs: &[(char, char)],
) -> StructuralSnapshot {
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut pairs = Vec::new();
    let mut unmatched = Vec::new();
    for (at, ch) in buffer.chars().enumerate() {
        if structural_pairs.iter().any(|(open, _)| *open == ch) {
            stack.push((at, ch));
        } else if let Some((_, open)) = stack.last().copied() {
            if structural_pairs
                .iter()
                .any(|(pair_open, close)| *pair_open == open && *close == ch)
            {
                let (open_at, _) = stack.pop().expect("last established a stack item");
                pairs.push(StructuralPair {
                    open: open_at,
                    close: at,
                    depth: u16::try_from(stack.len()).unwrap_or(u16::MAX),
                    parent: None,
                });
            } else if structural_pairs.iter().any(|(_, close)| *close == ch) {
                unmatched.push(at);
            }
        } else if structural_pairs.iter().any(|(_, close)| *close == ch) {
            unmatched.push(at);
        }
    }
    unmatched.extend(stack.into_iter().map(|(at, _)| at));
    pairs.sort_by_key(|pair| pair.open);
    assign_pair_parents(&mut pairs);
    let mut tokens = Vec::with_capacity(pairs.len() * 2 + unmatched.len());
    for (pair_index, pair) in pairs.iter().enumerate() {
        tokens.push(StructuralToken {
            at: pair.open,
            depth: pair.depth,
            matched: true,
            pair: Some(pair_index),
        });
        tokens.push(StructuralToken {
            at: pair.close,
            depth: pair.depth,
            matched: true,
            pair: Some(pair_index),
        });
    }
    tokens.extend(unmatched.into_iter().map(|at| StructuralToken {
        at,
        depth: 0,
        matched: false,
        pair: None,
    }));
    tokens.sort_by_key(|token| token.at);
    StructuralSnapshot {
        revision,
        pairs,
        tokens,
    }
}

pub(crate) fn update_plain_structural_snapshot(
    previous: &StructuralSnapshot,
    buffer: &Rope,
    revision: u64,
    structural_pairs: &[(char, char)],
    delta: &BufferDelta,
) -> (StructuralSnapshot, bool) {
    let BufferDelta::Edits(edits) = delta else {
        return (plain_structural_snapshot(buffer, revision, structural_pairs), false);
    };
    let can_remap = edits.iter().all(|edit| {
        let first_token = previous.tokens.partition_point(|token| token.at < edit.range.start);
        !edit
            .replacement
            .chars()
            .any(|ch| structural_pairs.iter().any(|(open, close)| *open == ch || *close == ch))
            && previous
                .tokens
                .get(first_token)
                .is_none_or(|token| token.at >= edit.range.end)
    });
    if !can_remap {
        return (plain_structural_snapshot(buffer, revision, structural_pairs), false);
    }

    let mut structure = previous.clone();
    remap_structural_snapshot(&mut structure, edits, revision);
    (structure, true)
}

fn assign_pair_parents(pairs: &mut [StructuralPair]) {
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..pairs.len() {
        while stack
            .last()
            .is_some_and(|parent| pairs[*parent].close < pairs[index].close)
        {
            stack.pop();
        }
        pairs[index].parent = stack.last().copied();
        pairs[index].depth = pairs[index]
            .parent
            .map_or(0, |parent| pairs[parent].depth.saturating_add(1));
        stack.push(index);
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

fn parse_rope(parser: &mut Parser, buffer: &Rope, old_tree: Option<&Tree>) -> Option<Tree> {
    let len_bytes = buffer.len_bytes();
    parser.parse_with_options(
        &mut |byte_offset, _| {
            if byte_offset >= len_bytes {
                return &[] as &[u8];
            }
            let (chunk, chunk_start, _, _) = buffer.chunk_at_byte(byte_offset);
            &chunk.as_bytes()[byte_offset - chunk_start..]
        },
        old_tree,
        None,
    )
}

fn edited_line_range(new_buffer: &Rope, edits: &[BufferEdit]) -> Option<Range<usize>> {
    let mut changed = None;
    let mut shift = 0isize;
    for edit in edits {
        let new_start = edit
            .range
            .start
            .saturating_add_signed(shift)
            .min(new_buffer.len_chars());
        let replacement_chars = edit.replacement.chars().count();
        let new_end = new_start.saturating_add(replacement_chars).min(new_buffer.len_chars());
        let start_line = new_buffer.char_to_line(new_start);
        let end_line = new_buffer.char_to_line(new_end).saturating_add(1);
        include_line_range(&mut changed, start_line..end_line);
        shift = shift.saturating_add(
            isize::try_from(replacement_chars).unwrap_or(isize::MAX)
                - isize::try_from(edit.range.end.saturating_sub(edit.range.start)).unwrap_or(isize::MAX),
        );
    }
    changed
}

fn include_line_range(target: &mut Option<Range<usize>>, addition: Range<usize>) {
    match target {
        Some(current) => {
            current.start = current.start.min(addition.start);
            current.end = current.end.max(addition.end);
        }
        None => *target = Some(addition),
    }
}

fn collect_rope_captures(
    grammar: GrammarId,
    tree: &Tree,
    source: &Rope,
    byte_range: Range<usize>,
    depth: u16,
    out: &mut Vec<CapturedSpan>,
) {
    let config = catalog::grammar(grammar);
    let injections = collect_rope_injection_matches(tree, source, config, byte_range.clone());
    let suppression: Vec<Range<usize>> = injections
        .iter()
        .filter(|injection| !injection.include_children)
        .map(|injection| injection.content_start..injection.content_end)
        .collect();

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range.clone());
    let mut matches = cursor.matches(&config.highlights, tree.root_node(), |node: tree_sitter::Node<'_>| {
        source.byte_slice(node.byte_range()).chunks().map(str::as_bytes)
    });
    while let Some(query_match) = matches.next() {
        let pattern_index = query_match.pattern_index as u16;
        for capture in query_match.captures {
            let Some(role) = config.capture_roles.get(capture.index as usize).copied().flatten() else {
                continue;
            };
            let node = capture.node;
            let start = node.start_byte();
            let end = node.end_byte();
            if start >= end || suppression.iter().any(|range| range.start <= start && end <= range.end) {
                continue;
            }
            out.push(CapturedSpan {
                start,
                end,
                role,
                depth,
                pattern_index,
            });
        }
    }

    for injection in injections {
        if injection.content_end <= injection.content_start {
            continue;
        }
        let content_range = injection.content_start..injection.content_end;
        let local_range = byte_range
            .start
            .max(content_range.start)
            .saturating_sub(content_range.start)
            ..byte_range
                .end
                .min(content_range.end)
                .saturating_sub(content_range.start);
        if local_range.is_empty() {
            continue;
        }
        let sub_source = source.byte_slice(content_range.clone()).to_string();
        let Some(sub_tree) = parse_sub_source(injection.embedded, sub_source.as_bytes()) else {
            continue;
        };
        collect_captures(
            injection.embedded,
            &sub_tree,
            sub_source.as_bytes(),
            injection.content_start,
            depth + 1,
            local_range,
            out,
        );
    }
}

fn collect_rope_injection_matches(
    tree: &Tree,
    source: &Rope,
    config: &catalog::GrammarConfig,
    byte_range: Range<usize>,
) -> Vec<InjectionMatch> {
    let Some(injections_query) = config.injections.as_ref() else {
        return Vec::new();
    };
    let content_index = config.injection_content_index;
    let language_index = config.injection_language_index;
    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range);
    let mut matches = cursor.matches(injections_query, tree.root_node(), |node: tree_sitter::Node<'_>| {
        source.byte_slice(node.byte_range()).chunks().map(str::as_bytes)
    });
    while let Some(query_match) = matches.next() {
        let mut content_node = None;
        let mut language_text = None;
        for capture in query_match.captures {
            if Some(capture.index) == content_index {
                content_node = Some(capture.node);
            } else if Some(capture.index) == language_index {
                language_text = Some(source.byte_slice(capture.node.byte_range()).to_string());
            }
        }
        let Some(node) = content_node else {
            continue;
        };
        let settings = injections_query.property_settings(query_match.pattern_index);
        let language_property = settings
            .iter()
            .find(|property| property.key.as_ref() == "injection.language")
            .and_then(|property| property.value.as_deref());
        let embedded = language_text
            .as_deref()
            .and_then(catalog::injectable_grammar)
            .or_else(|| language_property.and_then(catalog::injectable_grammar))
            .or(config.implicit_injection_grammar);
        let Some(embedded) = embedded else {
            continue;
        };
        let include_children = settings.iter().any(|property| {
            property.key.as_ref() == "injection.include-children"
                && match property.value.as_deref() {
                    None => true,
                    Some(value) => value.eq_ignore_ascii_case("true"),
                }
        });
        out.push(InjectionMatch {
            content_start: node.start_byte(),
            content_end: node.end_byte(),
            embedded,
            include_children,
        });
    }
    out
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
    byte_range: Range<usize>,
    out: &mut Vec<CapturedSpan>,
) {
    let config = catalog::grammar(grammar);

    let injections = collect_injection_matches(tree, source, config, byte_range.clone());
    let suppression: Vec<std::ops::Range<usize>> = injections
        .iter()
        .filter(|inj| !inj.include_children)
        .map(|inj| inj.content_start..inj.content_end)
        .collect();

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range.clone());
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
            byte_range
                .start
                .max(inj.content_start)
                .saturating_sub(inj.content_start)
                ..byte_range.end.min(inj.content_end).saturating_sub(inj.content_start),
            out,
        );
    }
}

fn collect_injection_matches(
    tree: &Tree,
    source: &[u8],
    config: &catalog::GrammarConfig,
    byte_range: Range<usize>,
) -> Vec<InjectionMatch> {
    let Some(injections_query) = config.injections.as_ref() else {
        return Vec::new();
    };
    let content_index = config.injection_content_index;
    let language_index = config.injection_language_index;

    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(byte_range);
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
    byte_range: Range<usize>,
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
        let start = cap.start.max(byte_range.start);
        let end = cap.end.min(byte_range.end);
        if start >= end {
            continue;
        }
        events.push(Event {
            byte: start,
            is_end: false,
            capture_ix: ix,
        });
        events.push(Event {
            byte: end,
            is_end: true,
            capture_ix: ix,
        });
    }
    if events.is_empty() {
        return;
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
fn line_bounds_from_rope(buffer: &Rope, lines: Range<usize>) -> (Vec<usize>, Vec<usize>) {
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut display_ends = Vec::with_capacity(lines.len());
    for line_ix in lines {
        let line_start = buffer.line_to_byte(line_ix);
        let line = buffer.line(line_ix);
        let mut trailing_break_bytes = 0;
        let mut char_ix = line.len_chars();
        while char_ix > 0 {
            let character = line.char(char_ix - 1);
            if !matches!(character, '\n' | '\r') {
                break;
            }
            trailing_break_bytes += character.len_utf8();
            char_ix -= 1;
        }
        line_starts.push(line_start);
        display_ends.push(line_start + line.len_bytes().saturating_sub(trailing_break_bytes));
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
