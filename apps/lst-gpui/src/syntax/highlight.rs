use super::{catalog, SyntaxLanguage, SyntaxSpan};
use crate::ui::theme::SyntaxRole;
use tree_sitter_highlight::{
    Highlight as TreeSitterHighlight, HighlightConfiguration,
    HighlightEvent as TreeSitterHighlightEvent, Highlighter as TreeSitterHighlighter,
};

pub(super) fn highlight_source(language: SyntaxLanguage, source: &str) -> Vec<Vec<SyntaxSpan>> {
    let (line_starts, display_ends) = line_bounds(source);
    let mut lines = vec![Vec::new(); line_starts.len()];
    let spec = catalog::syntax_spec(language);
    debug_assert!(catalog::required_injections_are_registered(&spec));
    let mut highlighter = TreeSitterHighlighter::new();
    let Ok(events) = highlighter.highlight(
        catalog::config(spec.root),
        source.as_bytes(),
        None,
        injection_config_for_highlighter,
    ) else {
        return lines;
    };

    let mut stack: Vec<TreeSitterHighlight> = Vec::new();
    for event in events {
        match event {
            Ok(TreeSitterHighlightEvent::HighlightStart(highlight)) => stack.push(highlight),
            Ok(TreeSitterHighlightEvent::HighlightEnd) => {
                let _ = stack.pop();
            }
            Ok(TreeSitterHighlightEvent::Source { start, end }) if start < end => {
                let Some(role) = stack
                    .last()
                    .and_then(|highlight| role_for_capture(highlight.0))
                else {
                    continue;
                };
                push_highlight_span(&mut lines, &line_starts, &display_ends, start, end, role);
            }
            Ok(TreeSitterHighlightEvent::Source { .. }) => {}
            Err(_) => return vec![Vec::new(); line_starts.len()],
        }
    }

    lines
}

fn injection_config_for_highlighter<'a>(name: &str) -> Option<&'a HighlightConfiguration> {
    catalog::injection_config(name)
}

fn role_for_capture(index: usize) -> Option<SyntaxRole> {
    let capture = catalog::capture_name(index)?;
    if capture.starts_with("comment") {
        Some(SyntaxRole::Comment)
    } else if capture.starts_with("string") {
        Some(SyntaxRole::String)
    } else if matches!(
        capture,
        "boolean" | "number" | "constant" | "constant.builtin"
    ) {
        Some(SyntaxRole::Constant)
    } else if capture.starts_with("function")
        || capture.starts_with("definition.function")
        || capture.starts_with("definition.method")
        || capture == "reference.call"
    {
        Some(SyntaxRole::Function)
    } else if capture.starts_with("keyword") {
        Some(SyntaxRole::Keyword)
    } else if capture == "operator" {
        Some(SyntaxRole::Operator)
    } else if capture.starts_with("type")
        || capture.starts_with("definition.class")
        || capture.starts_with("definition.interface")
        || capture == "reference.class"
        || capture == "reference.type"
    {
        Some(SyntaxRole::Type)
    } else if capture.starts_with("tag") {
        Some(SyntaxRole::Tag)
    } else if capture == "text.title" {
        Some(SyntaxRole::Title)
    } else if capture == "text.strong" {
        Some(SyntaxRole::Strong)
    } else if capture == "text.emphasis" {
        Some(SyntaxRole::Emphasis)
    } else if capture == "text.literal" {
        Some(SyntaxRole::Literal)
    } else if capture == "text.reference" || capture == "text.uri" {
        Some(SyntaxRole::Reference)
    } else if matches!(capture, "attribute" | "property" | "property.builtin") {
        Some(SyntaxRole::Property)
    } else if capture == "escape" || capture.starts_with("punctuation.special") {
        Some(SyntaxRole::Escape)
    } else if capture.starts_with("punctuation") {
        Some(SyntaxRole::Punctuation)
    } else if capture == "label" || capture == "module" || capture == "namespace" {
        Some(SyntaxRole::Label)
    } else {
        None
    }
}

fn line_bounds(source: &str) -> (Vec<usize>, Vec<usize>) {
    let mut line_starts = vec![0usize];
    let mut display_ends = Vec::new();
    let bytes = source.as_bytes();
    let mut ix = 0usize;
    let mut line_start = 0usize;

    while ix < bytes.len() {
        if bytes[ix] == b'\n' {
            let display_end = if ix > line_start && bytes[ix - 1] == b'\r' {
                ix - 1
            } else {
                ix
            };
            display_ends.push(display_end);
            ix += 1;
            line_start = ix;
            line_starts.push(line_start);
            continue;
        }
        ix += 1;
    }
    display_ends.push(source.strip_suffix('\r').map_or(source.len(), str::len));

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
        let line_ix = line_starts
            .partition_point(|offset| *offset <= start)
            .saturating_sub(1);
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
