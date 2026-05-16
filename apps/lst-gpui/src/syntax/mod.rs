mod catalog;
mod highlight;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SyntaxHighlightJobKey {
    pub(crate) language: SyntaxLanguage,
    pub(crate) revision: u64,
}

pub(crate) fn syntax_mode_for_language(language: Option<Language>) -> SyntaxMode {
    language
        .and_then(SyntaxLanguage::from_language)
        .map(SyntaxMode::TreeSitter)
        .unwrap_or(SyntaxMode::Plain)
}

pub(crate) fn compute_syntax_highlights(language: SyntaxLanguage, source: &str) -> Vec<Vec<SyntaxSpan>> {
    highlight::highlight_source(language, source)
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
