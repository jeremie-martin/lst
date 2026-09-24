mod catalog;
mod highlight;

pub(crate) use highlight::{
    plain_structural_snapshot, update_plain_structural_snapshot, StructuralPair, StructuralSnapshot, StructuralToken,
    SyntaxInvalidation, TabSyntaxState,
};

use crate::ui::theme::SyntaxRole;
use lst_editor::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SyntaxLanguage {
    Rust,
    Python,
    JavaScript,
    Jsx,
    TypeScript,
    Tsx,
    Json,
    Toml,
    Yaml,
    Markdown,
    Html,
    Css,
}

impl SyntaxLanguage {
    pub(crate) fn from_language(language: Language) -> Option<Self> {
        match language {
            Language::Rust => Some(Self::Rust),
            Language::Python => Some(Self::Python),
            Language::JavaScript => Some(Self::JavaScript),
            Language::Jsx => Some(Self::Jsx),
            Language::TypeScript => Some(Self::TypeScript),
            Language::Tsx => Some(Self::Tsx),
            Language::Json | Language::Jsonc => Some(Self::Json),
            Language::Toml => Some(Self::Toml),
            Language::Yaml => Some(Self::Yaml),
            Language::Markdown => Some(Self::Markdown),
            Language::Html => Some(Self::Html),
            Language::Css | Language::Scss => Some(Self::Css),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SyntaxMode {
    Plain,
    TreeSitter(SyntaxLanguage),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SyntaxSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) role: SyntaxRole,
}

#[derive(Clone)]
pub(crate) struct CachedSyntaxHighlights {
    pub(crate) language: SyntaxLanguage,
    pub(crate) revision: u64,
    pub(crate) lines: Vec<Vec<SyntaxSpan>>,
    /// Byte length of each line's *display* text (no trailing `\n` / `\r`) in
    /// the source that produced `lines`. The renderer guards span reuse with
    /// this so that stale highlights from a prior revision are only painted
    /// onto lines whose bytes are still identical — keeping char/UTF-8
    /// boundaries valid and avoiding visible misalignment on the edited line.
    pub(crate) line_byte_lens: Vec<u32>,
    /// Whether each line's spans and byte length have been materialized for
    /// `revision`. Full-document parses allocate the slots cheaply; viewport
    /// preparation fills only the visible working set.
    pub(crate) valid_lines: Vec<bool>,
}

/// Compiles the tree-sitter queries `language` highlights with, so the first
/// frame does not pay for them. Safe from any thread; later callers share
/// the compiled queries.
pub(crate) fn warm_grammar(language: SyntaxLanguage) {
    let _ = catalog::grammar(catalog::root_grammar(language));
    if language == SyntaxLanguage::Markdown {
        let _ = catalog::grammar(catalog::GrammarId::MarkdownInline);
    }
}

pub(crate) fn syntax_mode_for_language(language: Option<Language>) -> SyntaxMode {
    language
        .and_then(SyntaxLanguage::from_language)
        .map(SyntaxMode::TreeSitter)
        .unwrap_or(SyntaxMode::Plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::SyntaxRole;
    use lst_editor::{BufferDelta, BufferEdit};
    use ropey::Rope;
    use std::collections::HashSet;

    const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

    fn parse(language: SyntaxLanguage, source: &str) -> TabSyntaxState {
        TabSyntaxState::parse_initial(language, &Rope::from_str(source), 0).expect("language is supported")
    }

    fn spans(state: &TabSyntaxState) -> (Vec<Vec<SyntaxSpan>>, Vec<u32>) {
        state.compute_spans_for_lines(0..usize::MAX)
    }

    fn line_roles(state: &TabSyntaxState, line: usize) -> HashSet<SyntaxRole> {
        spans(state).0[line].iter().map(|span| span.role).collect()
    }

    fn token_positions(structure: &StructuralSnapshot) -> Vec<usize> {
        (0..structure.tokens.len())
            .map(|index| structure.token_position(index))
            .collect()
    }

    fn pair_positions(structure: &StructuralSnapshot) -> Vec<(usize, usize)> {
        (0..structure.pairs.len())
            .map(|index| structure.pair(index))
            .map(|pair| (pair.open, pair.close))
            .collect()
    }

    /// Replaces `len` characters of ASCII `text` at the first `marker`.
    fn edit_at(text: &str, marker: &str, len: usize, replacement: &str) -> BufferEdit {
        let at = text.find(marker).expect("marker is present");
        BufferEdit {
            range: at..at + len,
            replacement: replacement.to_string(),
        }
    }

    /// Applies ordered, disjoint edits expressed in `before` coordinates.
    fn apply(before: &Rope, edits: &[BufferEdit]) -> Rope {
        let mut after = before.clone();
        for edit in edits.iter().rev() {
            after.remove(edit.range.clone());
            after.insert(edit.range.start, &edit.replacement);
        }
        after
    }

    /// Updates a parse of `before` with `edits` and asserts that its
    /// highlights and structure match a fresh parse of the edited text.
    fn incremental_matches_fresh(
        language: SyntaxLanguage,
        before: &str,
        edits: Vec<BufferEdit>,
    ) -> (TabSyntaxState, SyntaxInvalidation) {
        let before = Rope::from_str(before);
        let after = apply(&before, &edits);
        let mut state = TabSyntaxState::parse_initial(language, &before, 0).unwrap();
        let invalidation = state.update(&after, BufferDelta::Edits(edits), 1);
        let fresh = TabSyntaxState::parse_initial(language, &after, 1).unwrap();
        assert_eq!(spans(&state), spans(&fresh));
        assert_eq!(*state.shared_structure().borrow(), *fresh.shared_structure().borrow());
        (state, invalidation)
    }

    /// Updates `structure`, a plain snapshot of `before`, with one edit and
    /// asserts that it matches a fresh scan. Returns whether it was remapped.
    fn plain_update_matches_fresh(
        structure: &mut StructuralSnapshot,
        before: &Rope,
        pairs: &[(char, char)],
        edit: BufferEdit,
    ) -> bool {
        let after = apply(before, std::slice::from_ref(&edit));
        let revision = structure.revision + 1;
        let remapped =
            update_plain_structural_snapshot(structure, &after, revision, pairs, &BufferDelta::Edits(vec![edit]));
        assert_eq!(*structure, plain_structural_snapshot(&after, revision, pairs));
        remapped
    }

    #[test]
    fn plain_delimiter_byte_scan_matches_the_character_scan() {
        // ASCII pairs take the byte scan; an absent non-ASCII pair selects
        // the character iterator. Both must report character offsets.
        let mut unicode_brackets = BRACKETS.to_vec();
        unicode_brackets.push(('«', '»'));
        let buffer = Rope::from_str("caf\u{e9} (\u{1F600}[x]) {\r\n \u{2014} }\n((a)\n");
        let structure = plain_structural_snapshot(&buffer, 7, BRACKETS);
        assert_eq!(structure, plain_structural_snapshot(&buffer, 7, &unicode_brackets));
        assert_eq!(pair_positions(&structure), [(5, 10), (7, 9), (12, 18), (21, 23)]);
        assert_eq!(structure.tokens.iter().filter(|token| !token.matched).count(), 1);

        let mut buffer = Rope::from_str(&"café (👩‍💻[e\u{301}]) {\r\ntext} <a> unmatched ] ((\n".repeat(150));
        for index in 0..300 {
            let at = (index * 137) % (buffer.len_chars() + 1);
            buffer.insert(at, if index % 2 == 0 { "λ\t[" } else { ")中" });
        }
        buffer.insert(
            buffer.len_chars() / 2,
            &"unmarked café 👩‍💻 e\u{301} text\r\n".repeat(300),
        );
        assert!(buffer.chunks().count() > 1);
        for pairs in [
            &[][..],
            &[('(', ')')][..],
            &[('(', ')'), ('[', ']')][..],
            BRACKETS,
            &[('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')][..],
        ] {
            let mut unicode_pairs = pairs.to_vec();
            unicode_pairs.push(('«', '»'));
            assert_eq!(
                plain_structural_snapshot(&buffer, 9, pairs),
                plain_structural_snapshot(&buffer, 9, &unicode_pairs),
                "pairs {pairs:?}",
            );
        }
    }

    #[test]
    fn language_variants_share_grammars_and_shells_stay_plain() {
        for (language, mode) in [
            (Some(Language::Jsonc), SyntaxMode::TreeSitter(SyntaxLanguage::Json)),
            (Some(Language::Scss), SyntaxMode::TreeSitter(SyntaxLanguage::Css)),
            (Some(Language::Shell), SyntaxMode::Plain),
            (Some(Language::Bash), SyntaxMode::Plain),
            (Some(Language::Zsh), SyntaxMode::Plain),
            (None, SyntaxMode::Plain),
        ] {
            assert_eq!(syntax_mode_for_language(language), mode, "{language:?}");
        }
    }

    #[test]
    fn rust_source_produces_keyword_and_string_spans() {
        let source = "fn main() { let s = \"hi\"; }\n";
        let state = parse(SyntaxLanguage::Rust, source);
        let (lines, byte_lens) = spans(&state);
        assert_eq!(lines.len(), 2);
        assert_eq!(byte_lens.len(), 2);
        assert_eq!(byte_lens[0] as usize, source.lines().next().unwrap().len());
        let roles = line_roles(&state, 0);
        assert!(roles.contains(&SyntaxRole::Keyword), "{roles:?}");
        assert!(roles.contains(&SyntaxRole::String), "{roles:?}");
    }

    #[test]
    fn structural_pairs_exclude_quotes_and_delimiters_inside_strings() {
        let source = "fn main() { let text = \"([{}])\"; call(1); }\n";
        let state = parse(SyntaxLanguage::Rust, source);
        let structure = state.shared_structure();
        let structure = structure.borrow();
        assert_eq!(structure.pairs.len(), 3, "{structure:?}");
        assert!(token_positions(&structure)
            .into_iter()
            .all(|at| !matches!(source.as_bytes()[at], b'"' | b'\'')));
    }

    #[test]
    fn tsx_angle_pairs_are_tags_not_comparison_operators() {
        let source = "const less = a < b; const view = <div>{less}</div>;\n";
        let state = parse(SyntaxLanguage::Tsx, source);
        let angle_positions: Vec<usize> = token_positions(&state.shared_structure().borrow())
            .into_iter()
            .filter(|&at| matches!(source.as_bytes()[at], b'<' | b'>'))
            .collect();
        let comparison = source.find('<').unwrap();
        assert!(!angle_positions.contains(&comparison), "{angle_positions:?}");
        assert_eq!(angle_positions.len(), 4, "{angle_positions:?}");
    }

    #[test]
    fn markdown_with_rust_fence_paints_both_layers() {
        let source = "# Title\n\n```rust\nfn main() {}\n```\n";
        let state = parse(SyntaxLanguage::Markdown, source);
        let title_roles = line_roles(&state, 0);
        assert!(
            title_roles.contains(&SyntaxRole::Title),
            "expected title on heading line, got {title_roles:?}"
        );
        // The injected rust grammar should color `fn` as a keyword.
        let fn_line_index = source.lines().position(|l| l.contains("fn main")).unwrap();
        let fn_roles = line_roles(&state, fn_line_index);
        assert!(
            fn_roles.contains(&SyntaxRole::Keyword),
            "expected rust keyword in injected fence, got {fn_roles:?}"
        );
    }

    #[test]
    fn markdown_fence_uses_injected_rust_for_structure_and_selection() {
        let source = "```rust\nfn fenced() { let text = \"([\"; call(1); }\n```\n";
        let state = parse(SyntaxLanguage::Markdown, source);
        let call_start = source.find("call(1)").unwrap();
        let call_open = call_start + "call".len();
        let call_close = call_start + "call(1".len();
        let structure = state.shared_structure();
        let structure = structure.borrow();
        assert!(pair_positions(&structure).contains(&(call_open, call_close)));

        let string_start = source.find("([\"").unwrap();
        assert!(token_positions(&structure)
            .into_iter()
            .all(|at| at != string_start && at != string_start + 1));

        let ranges = state.selection_ranges_at(&[call_start + 1]);
        assert!(
            ranges.contains(&(call_start..call_start + "call(1)".len())),
            "injected call expression missing from {ranges:?}"
        );
    }

    #[test]
    fn line_byte_lens_match_display_line_lengths_for_mixed_line_endings() {
        // Disagreement between line_bounds and EditorTab::display_line_from_rope
        // would silently disable the renderer's cache-reuse guard for these
        // lines. Mix LF, CRLF, lone CR, and a trailing line with no newline
        // to lock the trim semantics in step.
        let source = "a\nb\r\nc\rd\r\r\ne";
        let buffer = Rope::from_str(source);
        let (_lines, byte_lens) = spans(&parse(SyntaxLanguage::Rust, source));
        let display_lens: Vec<u32> = (0..buffer.len_lines())
            .map(|i| {
                let mut line = buffer.line(i).to_string();
                while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                line.len() as u32
            })
            .collect();
        assert_eq!(byte_lens, display_lens);
    }

    #[test]
    fn incremental_updates_match_fresh_parses_and_remap_only_unaffected_structure() {
        let macros = std::iter::once("// generated source\n".to_string())
            .chain((0..512).map(|item| format!("fn item_{item}() {{ println!(\"item {{}}\", {item}); }}\n")))
            .collect::<String>();
        let paragraphs = (0..1_000)
            .map(|line| format!("paragraph {line}: alpha [label](target) omega\n"))
            .collect::<String>();
        let quoted = "fn main() { let text = \"(\"; }\n";
        let expression = "```rust\nfn fenced() { let value = (1 + 2); }\n```\n";
        let fenced = "```rust\nfn fenced() { call(1); }\n```\n";
        let links = "alpha [label](target)\nbeta { value }\n";
        let rust_identifier = "fn main() {\n    let x = 1;\n}\n";
        for (case, language, before, edits, remapped) in [
            (
                "identifier insert",
                SyntaxLanguage::Rust,
                rust_identifier,
                vec![edit_at(rust_identifier, " = 1", 0, "x")],
                true,
            ),
            (
                "prefix before macro injections",
                SyntaxLanguage::Rust,
                macros.as_str(),
                vec![edit_at(&macros, "// generated", 0, "x")],
                true,
            ),
            // Removing a quote exposes a delimiter that was inside a string.
            (
                "quote removal",
                SyntaxLanguage::Rust,
                quoted,
                vec![edit_at(quoted, "\"", 1, "")],
                false,
            ),
            (
                "quoting an injected expression",
                SyntaxLanguage::Markdown,
                expression,
                vec![
                    edit_at(expression, "(1 + 2)", 0, "\""),
                    edit_at(expression, "; }", 0, "\""),
                ],
                false,
            ),
            (
                "injected identifier insert",
                SyntaxLanguage::Markdown,
                fenced,
                vec![edit_at(fenced, "() { call", 0, "x")],
                true,
            ),
            (
                "insert among many inline regions",
                SyntaxLanguage::Markdown,
                paragraphs.as_str(),
                vec![edit_at(&paragraphs, " 500:", 0, "x")],
                true,
            ),
            (
                "insert at an inline region's start",
                SyntaxLanguage::Markdown,
                links,
                vec![edit_at(links, "alpha", 0, "x")],
                true,
            ),
        ] {
            let (state, _) = incremental_matches_fresh(language, before, edits);
            assert_eq!(state.structure_was_remapped(), remapped, "{case}");
        }
    }

    #[test]
    fn incremental_highlights_match_fresh_parse_across_document_separators() {
        for separator in ["\n", "\r\n", "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"] {
            let source =
                format!("fn first() {{ let x = 1; }}{separator}fn second() {{ let y = 2; }}\nfn third() {{}}\n");
            let mut buffer = Rope::from_str(&source);
            let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, 0).unwrap();
            let (mut cached_lines, mut cached_lens) = spans(&state);
            let at = buffer.byte_to_char(source.find("let y").unwrap());
            for (revision, edit) in [
                BufferEdit {
                    range: at..at,
                    replacement: "/*".to_string(),
                },
                BufferEdit {
                    range: at..at + 2,
                    replacement: String::new(),
                },
                BufferEdit {
                    range: 0..0,
                    replacement: separator.to_string(),
                },
            ]
            .into_iter()
            .enumerate()
            {
                buffer = apply(&buffer, std::slice::from_ref(&edit));
                let invalidation = state.update(&buffer, BufferDelta::Edits(vec![edit]), revision as u64 + 1);
                match invalidation {
                    SyntaxInvalidation::Full | SyntaxInvalidation::LineTopology(_) => {
                        (cached_lines, cached_lens) = spans(&state)
                    }
                    SyntaxInvalidation::Lines(lines) => {
                        let (new_lines, new_lens) = state.compute_spans_for_lines(lines.clone());
                        cached_lines.splice(lines.clone(), new_lines);
                        cached_lens.splice(lines, new_lens);
                    }
                }
                let fresh = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, revision as u64 + 1).unwrap();
                assert_eq!(
                    (cached_lines.clone(), cached_lens.clone()),
                    spans(&fresh),
                    "{separator:?}, edit {revision}"
                );
                assert_eq!(
                    *state.shared_structure().borrow(),
                    *fresh.shared_structure().borrow(),
                    "{separator:?}, edit {revision}"
                );
            }
        }
    }

    #[test]
    fn ordinary_edit_recomputes_only_a_small_line_window() {
        let before = (0..200)
            .map(|line| format!("fn item_{line}() {{ let value_{line} = {line}; }}\n"))
            .collect::<String>();
        let (state, invalidation) =
            incremental_matches_fresh(SyntaxLanguage::Rust, &before, vec![edit_at(&before, "_100 =", 0, "x")]);
        let SyntaxInvalidation::Lines(changed_lines) = invalidation else {
            panic!("single-line typing should preserve line topology");
        };
        assert!(changed_lines.len() <= 4, "unexpected invalidation: {changed_lines:?}");
        let (lines, lens) = spans(&state);
        let (partial_lines, partial_lens) = state.compute_spans_for_lines(changed_lines.clone());
        assert_eq!(partial_lines, lines[changed_lines.clone()]);
        assert_eq!(partial_lens, lens[changed_lines]);
        assert!(state.structure_was_remapped());
    }

    #[test]
    fn line_topology_change_requests_a_full_highlight_rebuild() {
        let before = "fn first() {}\nfn second() {}\n";
        let (_, invalidation) = incremental_matches_fresh(
            SyntaxLanguage::Rust,
            before,
            vec![edit_at(before, "fn second", 0, "// inserted\n")],
        );
        assert!(invalidation.is_full());
    }

    #[test]
    fn plain_structure_updates_match_fresh_snapshots() {
        // Text between delimiters shifts them; removing a delimiter rescans.
        let text = "{ alpha }\n";
        let before = Rope::from_str(text);
        for (edit, remapped) in [
            (edit_at(text, "alpha", 0, "x"), true),
            (edit_at(text, "{", 1, ""), false),
        ] {
            let mut structure = plain_structural_snapshot(&before, 0, BRACKETS);
            assert_eq!(
                plain_update_matches_fresh(&mut structure, &before, BRACKETS, edit),
                remapped
            );
        }

        let before = Rope::from_str("café { base } tail");
        let mut unicode_brackets = BRACKETS.to_vec();
        unicode_brackets.push(('«', '»'));
        for pairs in [BRACKETS, &unicode_brackets[..]] {
            for replacement in ["ordinary é", "([中])", "«λ»"] {
                let mut structure = plain_structural_snapshot(&before, 0, pairs);
                let edit = BufferEdit {
                    range: 6..6,
                    replacement: replacement.to_string(),
                };
                plain_update_matches_fresh(&mut structure, &before, pairs, edit);
            }
        }
    }

    #[test]
    fn plain_edits_to_dense_structure_reuse_storage_and_shift_lazily() {
        let before = Rope::from_str(&"{}[]()\n".repeat(10_000));
        for at in [before.len_chars(), 0] {
            let mut structure = plain_structural_snapshot(&before, 0, BRACKETS);
            let storage = (structure.pairs.as_ptr(), structure.tokens.as_ptr());
            let last = structure.tokens.len() - 1;
            let raw_last = structure.tokens[last].at;
            let edit = BufferEdit {
                range: at..at,
                replacement: "x".to_string(),
            };
            assert!(
                plain_update_matches_fresh(&mut structure, &before, BRACKETS, edit),
                "edit at {at}"
            );
            assert_eq!(
                (structure.pairs.as_ptr(), structure.tokens.as_ptr()),
                storage,
                "edit at {at}"
            );
            // Stored positions stay put; token_position resolves the shift.
            assert_eq!(structure.tokens[last].at, raw_last, "edit at {at}");
        }
    }
}
