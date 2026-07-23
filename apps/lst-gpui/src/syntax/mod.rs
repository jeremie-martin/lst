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
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::Rust,
        Self::Python,
        Self::JavaScript,
        Self::Jsx,
        Self::TypeScript,
        Self::Tsx,
        Self::Json,
        Self::Toml,
        Self::Yaml,
        Self::Markdown,
        Self::Html,
        Self::Css,
    ];

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

pub(crate) fn syntax_mode_for_language(language: Option<Language>) -> SyntaxMode {
    language
        .and_then(SyntaxLanguage::from_language)
        .map(SyntaxMode::TreeSitter)
        .unwrap_or(SyntaxMode::Plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn syntax_mode_maps_core_extensions() {
        let cases = [
            ("example.rs", SyntaxLanguage::Rust),
            ("example.py", SyntaxLanguage::Python),
            ("example.pyw", SyntaxLanguage::Python),
            ("example.js", SyntaxLanguage::JavaScript),
            ("example.mjs", SyntaxLanguage::JavaScript),
            ("example.cjs", SyntaxLanguage::JavaScript),
            ("example.jsx", SyntaxLanguage::Jsx),
            ("example.ts", SyntaxLanguage::TypeScript),
            ("example.tsx", SyntaxLanguage::Tsx),
            ("example.json", SyntaxLanguage::Json),
            ("example.toml", SyntaxLanguage::Toml),
            ("example.yaml", SyntaxLanguage::Yaml),
            ("example.yml", SyntaxLanguage::Yaml),
            ("example.md", SyntaxLanguage::Markdown),
            ("example.markdown", SyntaxLanguage::Markdown),
            ("example.html", SyntaxLanguage::Html),
            ("example.htm", SyntaxLanguage::Html),
            ("example.css", SyntaxLanguage::Css),
        ];

        for (path, language) in cases {
            let detected = lst_editor::language::detect(Some(&PathBuf::from(path)), None);
            assert_eq!(syntax_mode_for_language(detected), SyntaxMode::TreeSitter(language));
        }
        let detected = lst_editor::language::detect(Some(&PathBuf::from("example.txt")), None);
        assert_eq!(syntax_mode_for_language(detected), SyntaxMode::Plain);
    }

    fn full_parse(language: SyntaxLanguage, source: &str) -> (Vec<Vec<SyntaxSpan>>, Vec<u32>) {
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(language, &buffer, 0).expect("language is supported");
        state.compute_spans()
    }

    #[test]
    fn rust_source_produces_keyword_and_string_spans() {
        let source = "fn main() { let s = \"hi\"; }\n";
        let (lines, byte_lens) = full_parse(SyntaxLanguage::Rust, source);
        assert_eq!(lines.len(), 2);
        assert_eq!(byte_lens.len(), 2);
        assert_eq!(byte_lens[0] as usize, source.lines().next().unwrap().len());
        let roles: std::collections::HashSet<_> = lines[0].iter().map(|s| s.role).collect();
        assert!(roles.contains(&crate::ui::theme::SyntaxRole::Keyword), "{:?}", roles);
        assert!(roles.contains(&crate::ui::theme::SyntaxRole::String), "{:?}", roles);
    }

    #[test]
    fn structural_pairs_exclude_quotes_and_delimiters_inside_strings() {
        let source = "fn main() { let text = \"([{}])\"; call(1); }\n";
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, 0).unwrap();
        let structure = state.structure();
        assert_eq!(structure.pairs.len(), 3, "{structure:?}");
        assert!(structure
            .tokens
            .iter()
            .all(|token| { !matches!(buffer.char(token.at), '\"' | '\'') }));
    }

    #[test]
    fn tsx_angle_pairs_are_tags_not_comparison_operators() {
        let source = "const less = a < b; const view = <div>{less}</div>;\n";
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Tsx, &buffer, 0).unwrap();
        let angle_positions: Vec<usize> = state
            .structure()
            .tokens
            .iter()
            .filter(|token| matches!(buffer.char(token.at), '<' | '>'))
            .map(|token| token.at)
            .collect();
        let comparison = source.chars().position(|ch| ch == '<').unwrap();
        assert!(!angle_positions.contains(&comparison), "{angle_positions:?}");
        assert_eq!(angle_positions.len(), 4, "{angle_positions:?}");
    }

    #[test]
    fn markdown_with_rust_fence_paints_both_layers() {
        let source = "# Title\n\n```rust\nfn main() {}\n```\n";
        let (lines, _) = full_parse(SyntaxLanguage::Markdown, source);
        let title_roles: std::collections::HashSet<_> = lines[0].iter().map(|s| s.role).collect();
        assert!(
            title_roles.contains(&crate::ui::theme::SyntaxRole::Title),
            "expected title on heading line, got {:?}",
            title_roles
        );
        // The injected rust grammar should color `fn` as a keyword.
        let fn_line_index = source.lines().position(|l| l.contains("fn main")).unwrap();
        let fn_roles: std::collections::HashSet<_> = lines[fn_line_index].iter().map(|s| s.role).collect();
        assert!(
            fn_roles.contains(&crate::ui::theme::SyntaxRole::Keyword),
            "expected rust keyword in injected fence, got {:?}",
            fn_roles
        );
    }

    #[test]
    fn markdown_fence_uses_injected_rust_for_structure_and_selection() {
        let source = "```rust\nfn fenced() { let text = \"([\"; call(1); }\n```\n";
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Markdown, &buffer, 0).unwrap();
        let call_start = source.find("call(1)").unwrap();
        let call_open = call_start + "call".len();
        let call_close = call_start + "call(1".len();
        assert!(state
            .structure()
            .pairs
            .iter()
            .any(|pair| pair.open == call_open && pair.close == call_close));

        let string_start = source.find("([\"").unwrap();
        assert!(state
            .structure()
            .tokens
            .iter()
            .all(|token| { token.at != string_start && token.at != string_start + 1 }));

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
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, 0).unwrap();
        let (_lines, byte_lens) = state.compute_spans();
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
    fn incremental_reparse_matches_fresh_parse_on_single_char_insert() {
        use lst_editor::{BufferDelta, BufferEdit};
        let before = "fn main() {\n    let x = 1;\n}\n";
        let after = "fn main() {\n    let xx = 1;\n}\n";
        let buffer_before = ropey::Rope::from_str(before);
        let buffer_after = ropey::Rope::from_str(after);

        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer_before, 0).unwrap();
        // Inserted one 'x' immediately after the 'x' in `let x`.
        let insert_at = before.find("let x").unwrap() + "let x".len();
        let edit = BufferEdit {
            range: insert_at..insert_at,
            replacement: "x".to_string(),
        };
        state.update(&buffer_after, BufferDelta::Edits(vec![edit]), 1);
        let (incremental_lines, incremental_lens) = state.compute_spans();

        let (fresh_lines, fresh_lens) = full_parse(SyntaxLanguage::Rust, after);
        assert_eq!(incremental_lens, fresh_lens);
        assert_eq!(incremental_lines, fresh_lines);
    }

    #[test]
    fn ordinary_edit_recomputes_only_a_small_line_window() {
        use lst_editor::{BufferDelta, BufferEdit};

        let before = (0..200)
            .map(|line| format!("fn item_{line}() {{ let value_{line} = {line}; }}\n"))
            .collect::<String>();
        let edit_start = before.find("value_100").unwrap() + "value_".len();
        let mut after = before.clone();
        after.insert(edit_start, 'x');
        let before_buffer = ropey::Rope::from_str(&before);
        let after_buffer = ropey::Rope::from_str(&after);
        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &before_buffer, 0).unwrap();

        let invalidation = state.update(
            &after_buffer,
            BufferDelta::Edits(vec![BufferEdit {
                range: edit_start..edit_start,
                replacement: "x".to_string(),
            }]),
            1,
        );
        let SyntaxInvalidation::Lines(changed_lines) = invalidation else {
            panic!("single-line typing should preserve line topology");
        };
        assert!(changed_lines.len() <= 4, "unexpected invalidation: {changed_lines:?}");

        let (partial_lines, partial_lens) = state.compute_spans_for_lines(changed_lines.clone());
        let (fresh_lines, fresh_lens) = full_parse(SyntaxLanguage::Rust, &after);
        assert_eq!(partial_lines, fresh_lines[changed_lines.clone()]);
        assert_eq!(partial_lens, fresh_lens[changed_lines]);
        assert!(state.structure_was_remapped());
        let fresh_state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &after_buffer, 1).unwrap();
        assert_eq!(&*state.structure(), &*fresh_state.structure());
    }

    #[test]
    fn prefix_edit_reuses_unchanged_macro_injection_structure() {
        use lst_editor::{BufferDelta, BufferEdit};

        let mut before = String::from("// generated source\n");
        for item in 0..512 {
            before.push_str(&format!("fn item_{item}() {{ println!(\"item {{}}\", {item}); }}\n"));
        }
        let after = format!("x{before}");
        let before_buffer = ropey::Rope::from_str(&before);
        let after_buffer = ropey::Rope::from_str(&after);
        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &before_buffer, 0).unwrap();

        state.update(
            &after_buffer,
            BufferDelta::Edits(vec![BufferEdit {
                range: 0..0,
                replacement: "x".to_string(),
            }]),
            1,
        );

        assert!(state.structure_was_remapped());
        let fresh_state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &after_buffer, 1).unwrap();
        assert_eq!(&*state.structure(), &*fresh_state.structure());
    }

    #[test]
    fn edit_that_changes_delimiter_syntax_rebuilds_structure() {
        use lst_editor::{BufferDelta, BufferEdit};

        let before = "fn main() { let text = \"(\"; }\n";
        let quote = before.find('\"').unwrap();
        let mut after = before.to_string();
        after.remove(quote);
        let before_buffer = ropey::Rope::from_str(before);
        let after_buffer = ropey::Rope::from_str(&after);
        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &before_buffer, 0).unwrap();

        state.update(
            &after_buffer,
            BufferDelta::Edits(vec![BufferEdit {
                range: quote..quote + 1,
                replacement: String::new(),
            }]),
            1,
        );

        let fresh_state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &after_buffer, 1).unwrap();
        assert_eq!(&*state.structure(), &*fresh_state.structure());
    }

    #[test]
    fn edit_inside_injected_rust_rebuilds_structure() {
        use lst_editor::{BufferDelta, BufferEdit};

        let before = "```rust\nfn fenced() { let value = (1 + 2); }\n```\n";
        let expression_start = before.find("(1 + 2)").unwrap();
        let expression_end = expression_start + "(1 + 2)".len();
        let after = format!(
            "{}\"{}\"{}",
            &before[..expression_start],
            &before[expression_start..expression_end],
            &before[expression_end..]
        );
        let before_buffer = ropey::Rope::from_str(before);
        let after_buffer = ropey::Rope::from_str(&after);
        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Markdown, &before_buffer, 0).unwrap();

        state.update(
            &after_buffer,
            BufferDelta::Edits(vec![
                BufferEdit {
                    range: expression_start..expression_start,
                    replacement: "\"".to_string(),
                },
                BufferEdit {
                    range: expression_end..expression_end,
                    replacement: "\"".to_string(),
                },
            ]),
            1,
        );

        assert!(!state.structure_was_remapped());
        let fresh_state = TabSyntaxState::parse_initial(SyntaxLanguage::Markdown, &after_buffer, 1).unwrap();
        assert_eq!(&*state.structure(), &*fresh_state.structure());
        let quoted_expression = after.find("(1 + 2)").unwrap();
        assert!(state
            .structure()
            .tokens
            .iter()
            .all(|token| token.at != quoted_expression && token.at != quoted_expression + "(1 + 2)".len() - 1));
    }

    #[test]
    fn plain_structure_updates_match_fresh_snapshots() {
        use lst_editor::{BufferDelta, BufferEdit};

        let pairs = &[('(', ')'), ('[', ']'), ('{', '}')];
        let before = ropey::Rope::from_str("{ alpha }\n");
        let mut structure = plain_structural_snapshot(&before, 0, pairs);
        let inserted = ropey::Rope::from_str("{ xalpha }\n");
        let was_remapped = update_plain_structural_snapshot(
            &mut structure,
            &inserted,
            1,
            pairs,
            &BufferDelta::Edits(vec![BufferEdit {
                range: 2..2,
                replacement: "x".to_string(),
            }]),
        );
        assert!(was_remapped);
        assert_eq!(structure, plain_structural_snapshot(&inserted, 1, pairs));

        let removed = ropey::Rope::from_str(" xalpha }\n");
        let was_remapped = update_plain_structural_snapshot(
            &mut structure,
            &removed,
            2,
            pairs,
            &BufferDelta::Edits(vec![BufferEdit {
                range: 0..1,
                replacement: String::new(),
            }]),
        );
        assert!(!was_remapped);
        assert_eq!(structure, plain_structural_snapshot(&removed, 2, pairs));
    }

    #[test]
    fn plain_append_after_dense_structure_reuses_snapshot_storage() {
        use lst_editor::{BufferDelta, BufferEdit};

        let pairs = &[('(', ')'), ('[', ']'), ('{', '}')];
        let before_text = "{}[]()\n".repeat(10_000);
        let before = ropey::Rope::from_str(&before_text);
        let mut structure = plain_structural_snapshot(&before, 0, pairs);
        let pairs_ptr = structure.pairs.as_ptr();
        let tokens_ptr = structure.tokens.as_ptr();
        let at = before.len_chars();
        let after = ropey::Rope::from_str(&(before_text + "x"));

        let was_remapped = update_plain_structural_snapshot(
            &mut structure,
            &after,
            1,
            pairs,
            &BufferDelta::Edits(vec![BufferEdit {
                range: at..at,
                replacement: "x".to_string(),
            }]),
        );

        assert!(was_remapped);
        assert_eq!(structure.revision, 1);
        assert_eq!(structure.pairs.as_ptr(), pairs_ptr);
        assert_eq!(structure.tokens.as_ptr(), tokens_ptr);
        assert_eq!(structure, plain_structural_snapshot(&after, 1, pairs));
    }

    #[test]
    fn plain_middle_edit_shifts_dense_structure_lazily() {
        use lst_editor::{BufferDelta, BufferEdit};

        let pairs = &[('(', ')'), ('[', ']'), ('{', '}')];
        let before_text = "{}[]()\n".repeat(10_000);
        let before = ropey::Rope::from_str(&before_text);
        let mut structure = plain_structural_snapshot(&before, 0, pairs);
        let pairs_ptr = structure.pairs.as_ptr();
        let tokens_ptr = structure.tokens.as_ptr();
        let last_token_index = structure.tokens.len() - 1;
        let raw_last_position = structure.tokens[last_token_index].at;
        let after = ropey::Rope::from_str(&format!("x{before_text}"));

        let was_remapped = update_plain_structural_snapshot(
            &mut structure,
            &after,
            1,
            pairs,
            &BufferDelta::Edits(vec![BufferEdit {
                range: 0..0,
                replacement: "x".to_string(),
            }]),
        );

        assert!(was_remapped);
        assert_eq!(structure.pairs.as_ptr(), pairs_ptr);
        assert_eq!(structure.tokens.as_ptr(), tokens_ptr);
        assert_eq!(structure.tokens[last_token_index].at, raw_last_position);
        assert_eq!(structure.token_position(last_token_index), raw_last_position + 1);
        assert_eq!(structure, plain_structural_snapshot(&after, 1, pairs));
    }

    #[test]
    fn line_topology_change_requests_a_full_highlight_rebuild() {
        use lst_editor::{BufferDelta, BufferEdit};

        let before = "fn first() {}\nfn second() {}\n";
        let insert_at = before.find("fn second").unwrap();
        let after = format!("{}// inserted\n{}", &before[..insert_at], &before[insert_at..]);
        let before_buffer = ropey::Rope::from_str(before);
        let after_buffer = ropey::Rope::from_str(&after);
        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &before_buffer, 0).unwrap();

        let invalidation = state.update(
            &after_buffer,
            BufferDelta::Edits(vec![BufferEdit {
                range: insert_at..insert_at,
                replacement: "// inserted\n".to_string(),
            }]),
            1,
        );

        assert_eq!(invalidation, SyntaxInvalidation::Full);
        assert_eq!(state.compute_spans(), full_parse(SyntaxLanguage::Rust, &after));
    }

    #[test]
    fn injected_grammars_are_not_root_syntax_modes() {
        assert_eq!(
            SyntaxLanguage::ALL,
            &[
                SyntaxLanguage::Rust,
                SyntaxLanguage::Python,
                SyntaxLanguage::JavaScript,
                SyntaxLanguage::Jsx,
                SyntaxLanguage::TypeScript,
                SyntaxLanguage::Tsx,
                SyntaxLanguage::Json,
                SyntaxLanguage::Toml,
                SyntaxLanguage::Yaml,
                SyntaxLanguage::Markdown,
                SyntaxLanguage::Html,
                SyntaxLanguage::Css,
            ]
        );
        assert_eq!(
            SyntaxLanguage::from_language(lst_editor::Language::Markdown),
            Some(SyntaxLanguage::Markdown)
        );
    }
}
