use super::SyntaxLanguage;
use std::sync::LazyLock;
use tree_sitter_highlight::HighlightConfiguration;

pub(super) const CAPTURE_NAMES: &[&str] = &[
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum GrammarId {
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
    MarkdownInline,
    Html,
    Css,
}

#[derive(Clone, Copy)]
pub(super) struct RequiredInjection {
    pub(super) name: &'static str,
    pub(super) grammar: GrammarId,
}

#[derive(Clone, Copy)]
pub(super) struct SyntaxSpec {
    pub(super) root: GrammarId,
    pub(super) required_injections: &'static [RequiredInjection],
}

const NO_REQUIRED_INJECTIONS: &[RequiredInjection] = &[];
const MARKDOWN_REQUIRED_INJECTIONS: &[RequiredInjection] = &[RequiredInjection {
    name: "markdown_inline",
    grammar: GrammarId::MarkdownInline,
}];

#[derive(Clone, Copy)]
struct InjectableGrammar {
    names: &'static [&'static str],
    grammar: GrammarId,
}

const INJECTABLE_GRAMMARS: &[InjectableGrammar] = &[
    InjectableGrammar {
        names: &["rust", "rs"],
        grammar: GrammarId::Rust,
    },
    InjectableGrammar {
        names: &["python", "py"],
        grammar: GrammarId::Python,
    },
    InjectableGrammar {
        names: &["javascript", "js"],
        grammar: GrammarId::JavaScript,
    },
    InjectableGrammar {
        names: &["jsx"],
        grammar: GrammarId::Jsx,
    },
    InjectableGrammar {
        names: &["typescript", "ts"],
        grammar: GrammarId::TypeScript,
    },
    InjectableGrammar {
        names: &["tsx"],
        grammar: GrammarId::Tsx,
    },
    InjectableGrammar {
        names: &["json"],
        grammar: GrammarId::Json,
    },
    InjectableGrammar {
        names: &["toml"],
        grammar: GrammarId::Toml,
    },
    InjectableGrammar {
        names: &["yaml", "yml"],
        grammar: GrammarId::Yaml,
    },
    InjectableGrammar {
        names: &["markdown", "md"],
        grammar: GrammarId::Markdown,
    },
    InjectableGrammar {
        names: &["markdown_inline", "markdown-inline"],
        grammar: GrammarId::MarkdownInline,
    },
    InjectableGrammar {
        names: &["html"],
        grammar: GrammarId::Html,
    },
    InjectableGrammar {
        names: &["css"],
        grammar: GrammarId::Css,
    },
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
    let highlights = format!(
        "{}\n{}",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    highlight_config(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        &highlights,
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
    let injections = markdown_block_injections_query();
    highlight_config(
        tree_sitter_md::LANGUAGE.into(),
        "markdown",
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        &injections,
        "",
    )
});

static MARKDOWN_INLINE_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    highlight_config(
        tree_sitter_md::INLINE_LANGUAGE.into(),
        "markdown_inline",
        tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        tree_sitter_md::INJECTION_QUERY_INLINE,
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

pub(super) fn syntax_spec(language: SyntaxLanguage) -> SyntaxSpec {
    SyntaxSpec {
        root: root_grammar(language),
        required_injections: required_injections(language),
    }
}

fn root_grammar(language: SyntaxLanguage) -> GrammarId {
    match language {
        SyntaxLanguage::Rust => GrammarId::Rust,
        SyntaxLanguage::Python => GrammarId::Python,
        SyntaxLanguage::JavaScript => GrammarId::JavaScript,
        SyntaxLanguage::Jsx => GrammarId::Jsx,
        SyntaxLanguage::TypeScript => GrammarId::TypeScript,
        SyntaxLanguage::Tsx => GrammarId::Tsx,
        SyntaxLanguage::Json => GrammarId::Json,
        SyntaxLanguage::Toml => GrammarId::Toml,
        SyntaxLanguage::Yaml => GrammarId::Yaml,
        SyntaxLanguage::Markdown => GrammarId::Markdown,
        SyntaxLanguage::Html => GrammarId::Html,
        SyntaxLanguage::Css => GrammarId::Css,
    }
}

fn required_injections(language: SyntaxLanguage) -> &'static [RequiredInjection] {
    match language {
        SyntaxLanguage::Markdown => MARKDOWN_REQUIRED_INJECTIONS,
        _ => NO_REQUIRED_INJECTIONS,
    }
}

pub(super) fn config(grammar: GrammarId) -> &'static HighlightConfiguration {
    match grammar {
        GrammarId::Rust => &RUST_CONFIG,
        GrammarId::Python => &PYTHON_CONFIG,
        GrammarId::JavaScript => &JAVASCRIPT_CONFIG,
        GrammarId::Jsx => &JSX_CONFIG,
        GrammarId::TypeScript => &TYPESCRIPT_CONFIG,
        GrammarId::Tsx => &TSX_CONFIG,
        GrammarId::Json => &JSON_CONFIG,
        GrammarId::Toml => &TOML_CONFIG,
        GrammarId::Yaml => &YAML_CONFIG,
        GrammarId::Markdown => &MARKDOWN_CONFIG,
        GrammarId::MarkdownInline => &MARKDOWN_INLINE_CONFIG,
        GrammarId::Html => &HTML_CONFIG,
        GrammarId::Css => &CSS_CONFIG,
    }
}

pub(super) fn injection_config(name: &str) -> Option<&'static HighlightConfiguration> {
    let normalized = name.to_ascii_lowercase();
    INJECTABLE_GRAMMARS
        .iter()
        .find(|entry| entry.names.contains(&normalized.as_str()))
        .map(|entry| config(entry.grammar))
}

pub(super) fn required_injections_are_registered(spec: &SyntaxSpec) -> bool {
    spec.required_injections.iter().all(|injection| {
        injection_config(injection.name)
            .is_some_and(|resolved| std::ptr::eq(resolved, config(injection.grammar)))
    })
}

pub(super) fn capture_name(index: usize) -> Option<&'static str> {
    CAPTURE_NAMES.get(index).copied()
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
    config.configure(CAPTURE_NAMES);
    config
}

fn markdown_block_injections_query() -> String {
    let inline_injection =
        "((inline) @injection.content\n  (#set! injection.language \"markdown_inline\"))";
    let inline_injection_with_children =
        "((inline) @injection.content\n  (#set! injection.language \"markdown_inline\")\n  (#set! injection.include-children))";
    let injections = tree_sitter_md::INJECTION_QUERY_BLOCK
        .replace(inline_injection, inline_injection_with_children);
    assert_ne!(
        injections,
        tree_sitter_md::INJECTION_QUERY_BLOCK,
        "tree-sitter-md markdown inline injection query changed"
    );
    injections
}

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;

    #[test]
    fn catalog_configs_and_required_injections_are_valid() {
        for language in SyntaxLanguage::ALL {
            let spec = syntax_spec(*language);
            let _ = config(spec.root);
            for injection in spec.required_injections {
                assert_eq!(
                    injection_config(injection.name).map(|_| ()),
                    Some(()),
                    "required injection {} is not registered",
                    injection.name
                );
                let _ = config(injection.grammar);
            }
        }

        for entry in INJECTABLE_GRAMMARS {
            let _ = config(entry.grammar);
        }
    }
}
