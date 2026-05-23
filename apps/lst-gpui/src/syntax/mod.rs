mod catalog;
mod highlight;

pub(crate) use highlight::TabSyntaxState;

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
    pub(crate) lines: Vec<Vec<SyntaxSpan>>,
    /// Byte length of each line's *display* text (no trailing `\n` / `\r`) in
    /// the source that produced `lines`. The renderer guards span reuse with
    /// this so that stale highlights from a prior revision are only painted
    /// onto lines whose bytes are still identical — keeping char/UTF-8
    /// boundaries valid and avoiding visible misalignment on the edited line.
    pub(crate) line_byte_lens: Vec<u32>,
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
        let state = TabSyntaxState::parse_initial(language, &buffer, source, 0).expect("language is supported");
        state.compute_spans(source)
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
    fn line_byte_lens_match_display_line_lengths_for_mixed_line_endings() {
        // Disagreement between line_bounds and EditorTab::display_line_from_rope
        // would silently disable the renderer's cache-reuse guard for these
        // lines. Mix LF, CRLF, lone CR, and a trailing line with no newline
        // to lock the trim semantics in step.
        let source = "a\nb\r\nc\rd\r\r\ne";
        let buffer = ropey::Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, source, 0).unwrap();
        let (_lines, byte_lens) = state.compute_spans(source);
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

        let mut state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer_before, before, 0).unwrap();
        // Inserted one 'x' immediately after the 'x' in `let x`.
        let insert_at = before.find("let x").unwrap() + "let x".len();
        let edit = BufferEdit {
            range: insert_at..insert_at,
            replacement: "x".to_string(),
        };
        state.update(&buffer_after, after, BufferDelta::Edits(vec![edit]), 1);
        let (incremental_lines, incremental_lens) = state.compute_spans(after);

        let (fresh_lines, fresh_lens) = full_parse(SyntaxLanguage::Rust, after);
        assert_eq!(incremental_lens, fresh_lens);
        assert_eq!(incremental_lines, fresh_lines);
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
