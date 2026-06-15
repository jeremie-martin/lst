use super::SyntaxLanguage;
use crate::ui::theme::SyntaxRole;
use std::sync::LazyLock;
use tree_sitter::{Language as TsLanguage, Query};

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

pub(super) fn root_grammar(language: SyntaxLanguage) -> GrammarId {
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

pub(super) struct GrammarConfig {
    pub(super) language: TsLanguage,
    pub(super) highlights: Query,
    pub(super) injections: Option<Query>,
    /// Map from highlights-query capture index to the editor's `SyntaxRole`.
    /// `None` entries are non-highlight captures (e.g. `local.scope`).
    pub(super) capture_roles: Vec<Option<SyntaxRole>>,
    /// Capture index for the `injection.language` capture in `injections`,
    /// when the query defines one. Used to look up the embedded language.
    pub(super) injection_language_index: Option<u32>,
    /// Capture index for the `injection.content` capture in `injections`.
    pub(super) injection_content_index: Option<u32>,
    /// For roots that always inject a single embedded grammar over their
    /// whole tree (markdown → markdown_inline via the (inline) node), this
    /// is the embedded grammar id used as a fallback when the injection
    /// query carries no `injection.language` capture.
    pub(super) implicit_injection_grammar: Option<GrammarId>,
}

impl GrammarConfig {
    fn new(
        language: TsLanguage,
        highlights_source: &str,
        injections_source: Option<&str>,
        implicit_injection_grammar: Option<GrammarId>,
    ) -> Self {
        let highlights = Query::new(&language, highlights_source).expect("embedded highlights query is valid");
        let capture_roles = highlights
            .capture_names()
            .iter()
            .map(|name| role_for_capture_name(name))
            .collect();
        let (injections, injection_language_index, injection_content_index) = match injections_source {
            None => (None, None, None),
            Some(source) => {
                let query = Query::new(&language, source).expect("embedded injections query is valid");
                let language_index = query.capture_index_for_name("injection.language");
                let content_index = query.capture_index_for_name("injection.content");
                (Some(query), language_index, content_index)
            }
        };
        Self {
            language,
            highlights,
            injections,
            capture_roles,
            injection_language_index,
            injection_content_index,
            implicit_injection_grammar,
        }
    }
}

static RUST_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_rust::LANGUAGE.into(),
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        Some(tree_sitter_rust::INJECTIONS_QUERY),
        None,
    )
});

static PYTHON_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_python::LANGUAGE.into(),
        tree_sitter_python::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

static JAVASCRIPT_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_javascript::LANGUAGE.into(),
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        Some(tree_sitter_javascript::INJECTIONS_QUERY),
        None,
    )
});

static JSX_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    let highlights = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    GrammarConfig::new(
        tree_sitter_javascript::LANGUAGE.into(),
        &highlights,
        Some(tree_sitter_javascript::INJECTIONS_QUERY),
        None,
    )
});

static TYPESCRIPT_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

static TSX_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    let highlights = format!(
        "{}\n{}",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    GrammarConfig::new(tree_sitter_typescript::LANGUAGE_TSX.into(), &highlights, None, None)
});

static JSON_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_json::LANGUAGE.into(),
        tree_sitter_json::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

static TOML_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_toml_ng::LANGUAGE.into(),
        tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

static YAML_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_yaml::LANGUAGE.into(),
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

static MARKDOWN_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    let injections = markdown_block_injections_query();
    GrammarConfig::new(
        tree_sitter_md::LANGUAGE.into(),
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        Some(&injections),
        None,
    )
});

static MARKDOWN_INLINE_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_md::INLINE_LANGUAGE.into(),
        tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        Some(tree_sitter_md::INJECTION_QUERY_INLINE),
        None,
    )
});

static HTML_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_html::LANGUAGE.into(),
        tree_sitter_html::HIGHLIGHTS_QUERY,
        Some(tree_sitter_html::INJECTIONS_QUERY),
        None,
    )
});

static CSS_CONFIG: LazyLock<GrammarConfig> = LazyLock::new(|| {
    GrammarConfig::new(
        tree_sitter_css::LANGUAGE.into(),
        tree_sitter_css::HIGHLIGHTS_QUERY,
        None,
        None,
    )
});

pub(super) fn grammar(id: GrammarId) -> &'static GrammarConfig {
    match id {
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

pub(super) fn injectable_grammar(name: &str) -> Option<GrammarId> {
    let normalized = name.to_ascii_lowercase();
    INJECTABLE_GRAMMARS
        .iter()
        .find(|entry| entry.names.contains(&normalized.as_str()))
        .map(|entry| entry.grammar)
}

fn role_for_capture_name(name: &str) -> Option<SyntaxRole> {
    if name.starts_with("comment") {
        Some(SyntaxRole::Comment)
    } else if name.starts_with("string") {
        Some(SyntaxRole::String)
    } else if matches!(name, "boolean" | "number" | "constant" | "constant.builtin") {
        Some(SyntaxRole::Constant)
    } else if name.starts_with("function")
        || name.starts_with("definition.function")
        || name.starts_with("definition.method")
        || name == "reference.call"
    {
        Some(SyntaxRole::Function)
    } else if name.starts_with("keyword") {
        Some(SyntaxRole::Keyword)
    } else if name == "operator" {
        Some(SyntaxRole::Operator)
    } else if name.starts_with("type")
        || name.starts_with("definition.class")
        || name.starts_with("definition.interface")
        || name == "reference.class"
        || name == "reference.type"
    {
        Some(SyntaxRole::Type)
    } else if name.starts_with("tag") {
        Some(SyntaxRole::Tag)
    } else if name == "text.title" {
        Some(SyntaxRole::Title)
    } else if name == "text.strong" {
        Some(SyntaxRole::Strong)
    } else if name == "text.emphasis" {
        Some(SyntaxRole::Emphasis)
    } else if name == "text.literal" {
        Some(SyntaxRole::Literal)
    } else if name == "text.reference" || name == "text.uri" {
        Some(SyntaxRole::Reference)
    } else if matches!(name, "attribute" | "property" | "property.builtin") {
        Some(SyntaxRole::Property)
    } else if name == "escape" || name.starts_with("punctuation.special") {
        Some(SyntaxRole::Escape)
    } else if name.starts_with("punctuation") {
        Some(SyntaxRole::Punctuation)
    } else if name == "label" || name == "module" || name == "namespace" {
        Some(SyntaxRole::Label)
    } else {
        None
    }
}

fn markdown_block_injections_query() -> String {
    let inline_injection = "((inline) @injection.content\n  (#set! injection.language \"markdown_inline\"))";
    let inline_injection_with_children = concat!(
        "((inline) @injection.content\n",
        "  (#set! injection.language \"markdown_inline\")\n",
        "  (#set! injection.include-children))"
    );
    let injections = tree_sitter_md::INJECTION_QUERY_BLOCK.replace(inline_injection, inline_injection_with_children);
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
    fn catalog_configs_build_for_every_language() {
        for language in SyntaxLanguage::ALL {
            let _ = grammar(root_grammar(*language));
        }
        let _ = grammar(GrammarId::MarkdownInline);
        // Force every injectable grammar too. Otherwise a future entry in
        // INJECTABLE_GRAMMARS that isn't also a SyntaxLanguage root would
        // skip catalog validation and panic at runtime on first injection
        // match instead of failing CI.
        for entry in INJECTABLE_GRAMMARS {
            let _ = grammar(entry.grammar);
        }
    }

    #[test]
    fn injectable_lookup_round_trip() {
        for entry in INJECTABLE_GRAMMARS {
            for name in entry.names {
                assert_eq!(injectable_grammar(name), Some(entry.grammar));
            }
        }
        assert_eq!(injectable_grammar("totally-not-a-language"), None);
    }
}
