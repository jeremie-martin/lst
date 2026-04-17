use crate::ui::theme::syntax as theme_syntax;
use lst_editor::Language;
use std::sync::LazyLock;
use tree_sitter_highlight::{
    Highlight as TreeSitterHighlight, HighlightConfiguration,
    HighlightEvent as TreeSitterHighlightEvent, Highlighter as TreeSitterHighlighter,
};

const TREE_SITTER_CAPTURE_NAMES: &[&str] = &[
    "_name",
    "attribute",
    "boolean",
    "character",
    "charset",
    "comment",
    "comment.documentation",
    "conditional",
    "constant",
    "constant.builtin",
    "constructor",
    "definition.class",
    "definition.constant",
    "definition.function",
    "definition.interface",
    "definition.macro",
    "definition.method",
    "definition.module",
    "doc",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "function.call",
    "function.macro",
    "function.method",
    "function.method.call",
    "glimmer",
    "import",
    "injection.content",
    "injection.language",
    "keyframes",
    "keyword",
    "keyword.directive",
    "label",
    "local.definition",
    "local.reference",
    "local.scope",
    "media",
    "module",
    "name",
    "namespace",
    "none",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "reference.call",
    "reference.class",
    "reference.implementation",
    "reference.type",
    "string",
    "string.documentation",
    "string.escape",
    "string.regex",
    "string.special",
    "string.special.key",
    "supports",
    "tag",
    "tag.error",
    "text.emphasis",
    "text.literal",
    "text.reference",
    "text.strong",
    "text.title",
    "text.uri",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

static RUST_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
});

static PYTHON_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        tree_sitter_python::HIGHLIGHTS_QUERY,
        "",
        "",
    )
});

static JAVASCRIPT_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
});

static JSX_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    let highlights = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    highlight_config(
        tree_sitter_javascript::LANGUAGE.into(),
        "jsx",
        &highlights,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
});

static TYPESCRIPT_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
});

static TSX_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
});

static JSON_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_json::LANGUAGE.into(),
        "json",
        tree_sitter_json::HIGHLIGHTS_QUERY,
        "",
        "",
    )
});

static TOML_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_toml_ng::LANGUAGE.into(),
        "toml",
        tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        "",
        "",
    )
});

static YAML_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_yaml::LANGUAGE.into(),
        "yaml",
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
});

static MARKDOWN_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_md::LANGUAGE.into(),
        "markdown",
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        tree_sitter_md::INJECTION_QUERY_BLOCK,
        "",
    )
});

static HTML_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_html::LANGUAGE.into(),
        "html",
        tree_sitter_html::HIGHLIGHTS_QUERY,
        tree_sitter_html::INJECTIONS_QUERY,
        "",
    )
});

static CSS_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_css::LANGUAGE.into(),
        "css",
        tree_sitter_css::HIGHLIGHTS_QUERY,
        "",
        "",
    )
});

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

    fn from_injection(language: &str) -> Option<Self> {
        match language.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" => Some(Self::Python),
            "javascript" | "js" => Some(Self::JavaScript),
            "jsx" => Some(Self::Jsx),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "markdown" | "md" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "css" => Some(Self::Css),
            _ => None,
        }
    }

    fn config(self) -> &'static HighlightConfiguration {
        match self {
            Self::Rust => &RUST_CONFIG,
            Self::Python => &PYTHON_CONFIG,
            Self::JavaScript => &JAVASCRIPT_CONFIG,
            Self::Jsx => &JSX_CONFIG,
            Self::TypeScript => &TYPESCRIPT_CONFIG,
            Self::Tsx => &TSX_CONFIG,
            Self::Json => &JSON_CONFIG,
            Self::Toml => &TOML_CONFIG,
            Self::Yaml => &YAML_CONFIG,
            Self::Markdown => &MARKDOWN_CONFIG,
            Self::Html => &HTML_CONFIG,
            Self::Css => &CSS_CONFIG,
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
    pub(crate) color: u32,
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

pub(crate) fn compute_syntax_highlights(
    language: SyntaxLanguage,
    source: &str,
) -> Vec<Vec<SyntaxSpan>> {
    highlight_source(language, source)
}

fn highlight_config(
    language: tree_sitter::Language,
    name: &'static str,
    highlights_query: &str,
    injections_query: &str,
    locals_query: &str,
) -> HighlightConfiguration {
    let mut config = HighlightConfiguration::new(
        language,
        name,
        highlights_query,
        injections_query,
        locals_query,
    )
    .unwrap_or_else(|error| panic!("embedded tree-sitter {name} highlight query invalid: {error}"));
    config.configure(TREE_SITTER_CAPTURE_NAMES);
    config
}

fn tree_sitter_color_for_capture(index: usize) -> Option<u32> {
    let capture = TREE_SITTER_CAPTURE_NAMES.get(index).copied()?;
    if capture.starts_with("comment") {
        Some(theme_syntax::COMMENT)
    } else if capture.starts_with("string") {
        Some(theme_syntax::STRING)
    } else if matches!(
        capture,
        "boolean" | "number" | "constant" | "constant.builtin"
    ) {
        Some(theme_syntax::CONSTANT)
    } else if capture.starts_with("function")
        || capture.starts_with("definition.function")
        || capture.starts_with("definition.method")
        || capture == "reference.call"
    {
        Some(theme_syntax::FUNCTION)
    } else if capture.starts_with("keyword") {
        Some(theme_syntax::KEYWORD)
    } else if capture == "operator" {
        Some(theme_syntax::OPERATOR)
    } else if capture.starts_with("type")
        || capture.starts_with("definition.class")
        || capture.starts_with("definition.interface")
        || capture == "reference.class"
        || capture == "reference.type"
    {
        Some(theme_syntax::TYPE)
    } else if capture.starts_with("tag") {
        Some(theme_syntax::TAG)
    } else if capture == "text.title" {
        Some(theme_syntax::TITLE)
    } else if capture == "text.strong" {
        Some(theme_syntax::STRONG)
    } else if capture == "text.emphasis" {
        Some(theme_syntax::EMPHASIS)
    } else if capture == "text.literal" {
        Some(theme_syntax::LITERAL)
    } else if capture == "text.reference" || capture == "text.uri" {
        Some(theme_syntax::REFERENCE)
    } else if matches!(capture, "attribute" | "property" | "property.builtin") {
        Some(theme_syntax::PROPERTY)
    } else if capture == "escape" || capture.starts_with("punctuation.special") {
        Some(theme_syntax::ESCAPE)
    } else if capture.starts_with("punctuation") {
        Some(theme_syntax::PUNCTUATION)
    } else if capture == "label" || capture == "module" || capture == "namespace" {
        Some(theme_syntax::LABEL)
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
    color: u32,
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
                color,
            });
        }

        if end <= next_line_start {
            break;
        }
        start = next_line_start;
    }
}

fn highlight_source(language: SyntaxLanguage, source: &str) -> Vec<Vec<SyntaxSpan>> {
    let (line_starts, display_ends) = line_bounds(source);
    let mut lines = vec![Vec::new(); line_starts.len()];
    let mut highlighter = TreeSitterHighlighter::new();
    let Ok(events) = highlighter.highlight(language.config(), source.as_bytes(), None, |name| {
        SyntaxLanguage::from_injection(name).map(SyntaxLanguage::config)
    }) else {
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
                let Some(color) = stack
                    .last()
                    .and_then(|highlight| tree_sitter_color_for_capture(highlight.0))
                else {
                    continue;
                };
                push_highlight_span(&mut lines, &line_starts, &display_ends, start, end, color);
            }
            Ok(TreeSitterHighlightEvent::Source { .. }) => {}
            Err(_) => return vec![Vec::new(); line_starts.len()],
        }
    }

    lines
}
