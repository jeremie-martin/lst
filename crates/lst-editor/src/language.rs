use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[rustfmt::skip]
pub enum Language {
    Rust, Python, JavaScript, Jsx, TypeScript, Tsx, Json, Jsonc, Toml, Yaml, Markdown, Html, Css,
    Scss, Shell, Bash, Zsh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndentStyle {
    Spaces { width: usize },
    Tabs { display_width: usize },
}

impl IndentStyle {
    pub const fn width(self) -> usize {
        match self {
            Self::Spaces { width } => width,
            Self::Tabs { display_width } => display_width,
        }
    }

    pub fn indent_unit(self) -> String {
        match self {
            Self::Spaces { width } => " ".repeat(width),
            Self::Tabs { .. } => "\t".to_string(),
        }
    }

    pub const fn uses_tabs(self) -> bool {
        matches!(self, Self::Tabs { .. })
    }
}

pub struct LanguageConfig {
    pub indent: IndentStyle,
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub auto_pairs: &'static [(char, char)],
    pub auto_pair_suppress_quotes: &'static [char],
    pub auto_dedent_closers: &'static [char],
}

const BLOCK_C: Option<(&str, &str)> = Some(("/*", "*/"));
const BLOCK_HTML: Option<(&str, &str)> = Some(("<!--", "-->"));
const PAIRS_BASIC: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\''), ('`', '`')];
const PAIRS_NO_SQ: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];
const PAIRS_ANGLE: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\''), ('`', '`'), ('<', '>')];
const CL_BR: &[char] = &['}'];
const CL_NO: &[char] = &[];
const SUP_SQ: &[char] = &['\''];
const SUP_NO: &[char] = &[];

const fn lc(indent: IndentStyle, line_comment: Option<&'static str>, block_comment: Option<(&'static str, &'static str)>, auto_pairs: &'static [(char, char)], auto_pair_suppress_quotes: &'static [char], auto_dedent_closers: &'static [char]) -> LanguageConfig {
    LanguageConfig { indent, line_comment, block_comment, auto_pairs, auto_pair_suppress_quotes, auto_dedent_closers }
}

const fn sp(width: usize) -> IndentStyle {
    IndentStyle::Spaces { width }
}

#[rustfmt::skip]
const CONFIGS: &[LanguageConfig] = &[
    lc(sp(4), Some("//"), BLOCK_C,    PAIRS_NO_SQ, SUP_SQ, CL_BR), // Rust
    lc(sp(4), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Python
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_BASIC, SUP_NO, CL_BR), // JavaScript
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_ANGLE, SUP_NO, CL_BR), // Jsx
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_BASIC, SUP_NO, CL_BR), // TypeScript
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_ANGLE, SUP_NO, CL_BR), // Tsx
    lc(sp(2), None,       None,       PAIRS_BASIC, SUP_NO, CL_BR), // Json
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_BASIC, SUP_NO, CL_BR), // Jsonc
    lc(sp(4), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Toml
    lc(sp(2), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Yaml
    lc(sp(2), None,       BLOCK_HTML, PAIRS_BASIC, SUP_NO, CL_NO), // Markdown
    lc(sp(2), None,       BLOCK_HTML, PAIRS_ANGLE, SUP_NO, CL_NO), // Html
    lc(sp(2), None,       BLOCK_C,    PAIRS_BASIC, SUP_NO, CL_BR), // Css
    lc(sp(2), Some("//"), BLOCK_C,    PAIRS_BASIC, SUP_NO, CL_BR), // Scss
    lc(sp(4), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Shell
    lc(sp(4), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Bash
    lc(sp(4), Some("#"),  None,       PAIRS_BASIC, SUP_NO, CL_NO), // Zsh
];

impl Language {
    pub fn config(self) -> &'static LanguageConfig {
        &CONFIGS[self as usize]
    }
}

pub fn detect(path: Option<&Path>, first_line: Option<&str>) -> Option<Language> {
    if let Some(path) = path {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(detect_from_filename) {
            return Some(name);
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()).and_then(detect_from_extension) {
            return Some(ext);
        }
    }
    first_line.and_then(detect_from_shebang)
}

#[rustfmt::skip]
fn detect_from_filename(name: &str) -> Option<Language> {
    Some(match name {
        "Cargo.lock" | "Cargo.toml" | "rust-toolchain.toml" | "pyproject.toml" => Language::Toml,
        ".bashrc" | ".bash_profile" | ".bash_login" | ".bash_logout" | ".bash_aliases" => Language::Bash,
        ".zshrc" | ".zprofile" | ".zlogin" | ".zlogout" | ".zshenv" => Language::Zsh,
        ".profile" | ".login" => Language::Shell,
        _ => return None,
    })
}

#[rustfmt::skip]
fn detect_from_extension(extension: &str) -> Option<Language> {
    Some(match extension.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "rs" => Language::Rust,
        "py" | "pyw" | "pyi" => Language::Python,
        "js" | "mjs" | "cjs" => Language::JavaScript,
        "jsx" => Language::Jsx,
        "ts" | "mts" | "cts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "json" => Language::Json,
        "jsonc" | "json5" => Language::Jsonc,
        "toml" => Language::Toml,
        "yaml" | "yml" => Language::Yaml,
        "md" | "markdown" | "mdx" => Language::Markdown,
        "html" | "htm" => Language::Html,
        "css" => Language::Css,
        "scss" | "sass" => Language::Scss,
        "sh" => Language::Shell,
        "bash" => Language::Bash,
        "zsh" => Language::Zsh,
        _ => return None,
    })
}

fn detect_from_shebang(first_line: &str) -> Option<Language> {
    let rest = first_line.strip_prefix("#!")?.trim_start();
    let (head, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let interpreter = if head.ends_with("/env") || head == "env" { tail.split_whitespace().next().unwrap_or("") } else { head.rsplit('/').next().unwrap_or("") }.trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '.');
    match interpreter {
        "python" | "py" => Some(Language::Python),
        "node" => Some(Language::JavaScript),
        "bash" => Some(Language::Bash),
        "zsh" => Some(Language::Zsh),
        "sh" => Some(Language::Shell),
        _ => None,
    }
}

const CONFIG_DEFAULT: LanguageConfig = lc(sp(4), None, None, PAIRS_BASIC, SUP_NO, CL_BR);

pub(crate) const DEFAULT_CONFIG: &LanguageConfig = &CONFIG_DEFAULT;

pub(crate) fn config_for(lang: Option<Language>) -> &'static LanguageConfig {
    lang.map_or(DEFAULT_CONFIG, Language::config)
}
