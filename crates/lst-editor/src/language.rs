use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    Jsx,
    TypeScript,
    Tsx,
    Json,
    Jsonc,
    Toml,
    Yaml,
    Markdown,
    Html,
    Xml,
    Css,
    Scss,
    C,
    Cpp,
    Java,
    Go,
    CSharp,
    Swift,
    Kotlin,
    Scala,
    Zig,
    Shell,
    Bash,
    Zsh,
    Fish,
    Ruby,
    Perl,
    Lua,
    Sql,
    Haskell,
    Elixir,
    Erlang,
    Clojure,
    CommonLisp,
    Scheme,
    EmacsLisp,
    Dockerfile,
    Makefile,
    CMake,
    Ini,
    Proto,
    Vim,
    Tex,
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

// Shared auto-pair sets.
const PAIRS_BASIC: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const PAIRS_NO_SINGLE_QUOTE: &[(char, char)] =
    &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];
const PAIRS_WITH_ANGLE: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
    ('<', '>'),
];

const CLOSERS_BRACE: &[char] = &['}'];
const CLOSERS_NONE: &[char] = &[];

const SUPPRESS_SINGLE_QUOTE: &[char] = &['\''];
const SUPPRESS_NONE: &[char] = &[];

const CONFIG_RUST: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_NO_SINGLE_QUOTE,
    auto_pair_suppress_quotes: SUPPRESS_SINGLE_QUOTE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_PYTHON: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_JAVASCRIPT: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_JSX: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_WITH_ANGLE,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_TYPESCRIPT: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_TSX: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_WITH_ANGLE,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_JSON: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: None,
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_JSONC: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_TOML: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_YAML: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_MARKDOWN: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: None,
    block_comment: BLOCK_HTML,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_HTML: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: None,
    block_comment: BLOCK_HTML,
    auto_pairs: PAIRS_WITH_ANGLE,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_XML: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: None,
    block_comment: BLOCK_HTML,
    auto_pairs: PAIRS_WITH_ANGLE,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_CSS: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: None,
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_SCSS: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_C: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_CPP: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_JAVA: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_GO: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Tabs { display_width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_CSHARP: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_SWIFT: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_KOTLIN: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_SCALA: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_ZIG: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("//"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_SHELL: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_BASH: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_ZSH: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_FISH: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_RUBY: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("#"),
    block_comment: Some(("=begin", "=end")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_PERL: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_LUA: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("--"),
    block_comment: Some(("--[[", "]]")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_SQL: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("--"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_HASKELL: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("--"),
    block_comment: Some(("{-", "-}")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_ELIXIR: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_ERLANG: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("%"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_CLOJURE: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some(";;"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_COMMONLISP: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some(";;"),
    block_comment: Some(("#|", "|#")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_SCHEME: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some(";;"),
    block_comment: Some(("#|", "|#")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_EMACSLISP: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some(";;"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_DOCKERFILE: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_MAKEFILE: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Tabs { display_width: 4 },
    line_comment: Some("#"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_CMAKE: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("#"),
    block_comment: Some(("#[[", "]]")),
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_INI: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: Some(";"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_PROTO: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("//"),
    block_comment: BLOCK_C,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

const CONFIG_VIM: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("\""),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_NONE,
};

const CONFIG_TEX: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 2 },
    line_comment: Some("%"),
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

// Used when a tab has no detected language (unknown extension, scratchpad).
// The values mirror the editor's prior hardcoded behavior so existing flows
// keep working even when detection returns None.
const CONFIG_DEFAULT: LanguageConfig = LanguageConfig {
    indent: IndentStyle::Spaces { width: 4 },
    line_comment: None,
    block_comment: None,
    auto_pairs: PAIRS_BASIC,
    auto_pair_suppress_quotes: SUPPRESS_NONE,
    auto_dedent_closers: CLOSERS_BRACE,
};

impl Language {
    pub const fn config(self) -> &'static LanguageConfig {
        match self {
            Self::Rust => &CONFIG_RUST,
            Self::Python => &CONFIG_PYTHON,
            Self::JavaScript => &CONFIG_JAVASCRIPT,
            Self::Jsx => &CONFIG_JSX,
            Self::TypeScript => &CONFIG_TYPESCRIPT,
            Self::Tsx => &CONFIG_TSX,
            Self::Json => &CONFIG_JSON,
            Self::Jsonc => &CONFIG_JSONC,
            Self::Toml => &CONFIG_TOML,
            Self::Yaml => &CONFIG_YAML,
            Self::Markdown => &CONFIG_MARKDOWN,
            Self::Html => &CONFIG_HTML,
            Self::Xml => &CONFIG_XML,
            Self::Css => &CONFIG_CSS,
            Self::Scss => &CONFIG_SCSS,
            Self::C => &CONFIG_C,
            Self::Cpp => &CONFIG_CPP,
            Self::Java => &CONFIG_JAVA,
            Self::Go => &CONFIG_GO,
            Self::CSharp => &CONFIG_CSHARP,
            Self::Swift => &CONFIG_SWIFT,
            Self::Kotlin => &CONFIG_KOTLIN,
            Self::Scala => &CONFIG_SCALA,
            Self::Zig => &CONFIG_ZIG,
            Self::Shell => &CONFIG_SHELL,
            Self::Bash => &CONFIG_BASH,
            Self::Zsh => &CONFIG_ZSH,
            Self::Fish => &CONFIG_FISH,
            Self::Ruby => &CONFIG_RUBY,
            Self::Perl => &CONFIG_PERL,
            Self::Lua => &CONFIG_LUA,
            Self::Sql => &CONFIG_SQL,
            Self::Haskell => &CONFIG_HASKELL,
            Self::Elixir => &CONFIG_ELIXIR,
            Self::Erlang => &CONFIG_ERLANG,
            Self::Clojure => &CONFIG_CLOJURE,
            Self::CommonLisp => &CONFIG_COMMONLISP,
            Self::Scheme => &CONFIG_SCHEME,
            Self::EmacsLisp => &CONFIG_EMACSLISP,
            Self::Dockerfile => &CONFIG_DOCKERFILE,
            Self::Makefile => &CONFIG_MAKEFILE,
            Self::CMake => &CONFIG_CMAKE,
            Self::Ini => &CONFIG_INI,
            Self::Proto => &CONFIG_PROTO,
            Self::Vim => &CONFIG_VIM,
            Self::Tex => &CONFIG_TEX,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" => Some(Self::Python),
            "javascript" | "js" => Some(Self::JavaScript),
            "jsx" => Some(Self::Jsx),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "json" => Some(Self::Json),
            "jsonc" | "json5" => Some(Self::Jsonc),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "markdown" | "md" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "xml" => Some(Self::Xml),
            "css" => Some(Self::Css),
            "scss" => Some(Self::Scss),
            "c" => Some(Self::C),
            "cpp" | "c++" | "cc" | "cxx" => Some(Self::Cpp),
            "java" => Some(Self::Java),
            "go" | "golang" => Some(Self::Go),
            "c_sharp" | "csharp" | "c#" | "cs" => Some(Self::CSharp),
            "swift" => Some(Self::Swift),
            "kotlin" | "kt" => Some(Self::Kotlin),
            "scala" => Some(Self::Scala),
            "zig" => Some(Self::Zig),
            "shell" | "sh" => Some(Self::Shell),
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            "ruby" | "rb" => Some(Self::Ruby),
            "perl" | "pl" => Some(Self::Perl),
            "lua" => Some(Self::Lua),
            "sql" => Some(Self::Sql),
            "haskell" | "hs" => Some(Self::Haskell),
            "elixir" | "ex" | "exs" => Some(Self::Elixir),
            "erlang" | "erl" => Some(Self::Erlang),
            "clojure" | "clj" | "cljs" => Some(Self::Clojure),
            "common_lisp" | "commonlisp" | "lisp" | "cl" => Some(Self::CommonLisp),
            "scheme" | "scm" | "racket" | "rkt" => Some(Self::Scheme),
            "emacs_lisp" | "emacslisp" | "elisp" | "el" => Some(Self::EmacsLisp),
            "dockerfile" => Some(Self::Dockerfile),
            "makefile" | "make" => Some(Self::Makefile),
            "cmake" => Some(Self::CMake),
            "ini" | "conf" | "cfg" => Some(Self::Ini),
            "proto" | "protobuf" => Some(Self::Proto),
            "vim" | "vimscript" => Some(Self::Vim),
            "tex" | "latex" => Some(Self::Tex),
            _ => None,
        }
    }
}

pub fn detect(path: Option<&Path>, first_line: Option<&str>) -> Option<Language> {
    if let Some(path) = path {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(lang) = detect_from_filename(name) {
                return Some(lang);
            }
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(lang) = detect_from_extension(ext) {
                return Some(lang);
            }
        }
    }
    first_line.and_then(detect_from_shebang)
}

pub fn detect_from_filename(name: &str) -> Option<Language> {
    match name {
        "Makefile" | "makefile" | "GNUmakefile" | "BSDmakefile" => Some(Language::Makefile),
        "Dockerfile" | "dockerfile" | "Containerfile" => Some(Language::Dockerfile),
        "CMakeLists.txt" => Some(Language::CMake),
        "Cargo.lock" | "Cargo.toml" | "rust-toolchain.toml" | "pyproject.toml" => {
            Some(Language::Toml)
        }
        ".bashrc" | ".bash_profile" | ".bash_login" | ".bash_logout" | ".bash_aliases" => {
            Some(Language::Bash)
        }
        ".zshrc" | ".zprofile" | ".zlogin" | ".zlogout" | ".zshenv" => Some(Language::Zsh),
        ".profile" | ".login" => Some(Language::Shell),
        _ => None,
    }
}

pub fn detect_from_extension(extension: &str) -> Option<Language> {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => Some(Language::Rust),
        "py" | "pyw" | "pyi" => Some(Language::Python),
        "js" | "mjs" | "cjs" => Some(Language::JavaScript),
        "jsx" => Some(Language::Jsx),
        "ts" | "mts" | "cts" => Some(Language::TypeScript),
        "tsx" => Some(Language::Tsx),
        "json" => Some(Language::Json),
        "jsonc" | "json5" => Some(Language::Jsonc),
        "toml" => Some(Language::Toml),
        "yaml" | "yml" => Some(Language::Yaml),
        "md" | "markdown" | "mdx" => Some(Language::Markdown),
        "html" | "htm" => Some(Language::Html),
        "xml" | "xhtml" | "svg" => Some(Language::Xml),
        "css" => Some(Language::Css),
        "scss" | "sass" => Some(Language::Scss),
        "c" | "h" => Some(Language::C),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "c++" | "h++" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "go" => Some(Language::Go),
        "cs" => Some(Language::CSharp),
        "swift" => Some(Language::Swift),
        "kt" | "kts" => Some(Language::Kotlin),
        "scala" | "sc" => Some(Language::Scala),
        "zig" => Some(Language::Zig),
        "sh" => Some(Language::Shell),
        "bash" => Some(Language::Bash),
        "zsh" => Some(Language::Zsh),
        "fish" => Some(Language::Fish),
        "rb" | "ruby" => Some(Language::Ruby),
        "pl" | "pm" => Some(Language::Perl),
        "lua" => Some(Language::Lua),
        "sql" => Some(Language::Sql),
        "hs" | "lhs" => Some(Language::Haskell),
        "ex" | "exs" => Some(Language::Elixir),
        "erl" | "hrl" => Some(Language::Erlang),
        "clj" | "cljs" | "cljc" | "edn" => Some(Language::Clojure),
        "lisp" | "cl" | "asd" => Some(Language::CommonLisp),
        "scm" | "ss" | "rkt" => Some(Language::Scheme),
        "el" => Some(Language::EmacsLisp),
        "dockerfile" => Some(Language::Dockerfile),
        "mk" => Some(Language::Makefile),
        "cmake" => Some(Language::CMake),
        "ini" | "conf" | "cfg" | "properties" => Some(Language::Ini),
        "proto" => Some(Language::Proto),
        "vim" | "vimrc" => Some(Language::Vim),
        "tex" | "latex" | "sty" | "cls" => Some(Language::Tex),
        _ => None,
    }
}

pub fn detect_from_shebang(first_line: &str) -> Option<Language> {
    let rest = first_line.strip_prefix("#!")?;
    let rest = rest.trim_start();
    let (head, tail) = match rest.split_once(char::is_whitespace) {
        Some((head, tail)) => (head, tail.trim_start()),
        None => (rest, ""),
    };
    let interpreter = if head.ends_with("/env") || head == "env" {
        tail.split_whitespace().next().unwrap_or("")
    } else {
        head.rsplit('/').next().unwrap_or("")
    };
    let interpreter = interpreter.trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '.');
    if let Some(lang) = Language::from_name(interpreter) {
        return Some(lang);
    }
    match interpreter {
        "node" => Some(Language::JavaScript),
        _ => None,
    }
}

pub(crate) const DEFAULT_CONFIG: &LanguageConfig = &CONFIG_DEFAULT;

pub(crate) fn config_for(lang: Option<Language>) -> &'static LanguageConfig {
    match lang {
        Some(l) => l.config(),
        None => DEFAULT_CONFIG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_matches_extension() {
        assert_eq!(
            detect(Some(&PathBuf::from("foo.rs")), None),
            Some(Language::Rust)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("foo.py")), None),
            Some(Language::Python)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("foo.mjs")), None),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("foo.tsx")), None),
            Some(Language::Tsx)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("foo.yml")), None),
            Some(Language::Yaml)
        );
    }

    #[test]
    fn detect_matches_whole_filename() {
        assert_eq!(
            detect(Some(&PathBuf::from("Makefile")), None),
            Some(Language::Makefile)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("GNUmakefile")), None),
            Some(Language::Makefile)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("Dockerfile")), None),
            Some(Language::Dockerfile)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("CMakeLists.txt")), None),
            Some(Language::CMake)
        );
        assert_eq!(
            detect(Some(&PathBuf::from(".bashrc")), None),
            Some(Language::Bash)
        );
        assert_eq!(
            detect(Some(&PathBuf::from("Cargo.toml")), None),
            Some(Language::Toml)
        );
    }

    #[test]
    fn detect_matches_shebang() {
        assert_eq!(
            detect(None, Some("#!/usr/bin/env python")),
            Some(Language::Python)
        );
        assert_eq!(
            detect(None, Some("#!/usr/bin/env python3")),
            Some(Language::Python)
        );
        assert_eq!(detect(None, Some("#!/bin/bash")), Some(Language::Bash));
        assert_eq!(
            detect(None, Some("#!/usr/bin/env node -v")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect(None, Some("#!/usr/bin/env ruby")),
            Some(Language::Ruby)
        );
        assert_eq!(
            detect(None, Some("#!/usr/bin/env lua")),
            Some(Language::Lua)
        );
    }

    #[test]
    fn detect_filename_beats_extension() {
        // A file literally named Dockerfile (without extension) resolves via filename.
        // A path like foo.dockerfile should resolve via extension to Dockerfile too.
        assert_eq!(
            detect(Some(&PathBuf::from("Dockerfile")), None),
            Some(Language::Dockerfile)
        );
        // Normal extension fallback still fires when the filename is not registered.
        assert_eq!(
            detect(Some(&PathBuf::from("Makefile.rs")), None),
            Some(Language::Rust)
        );
    }

    #[test]
    fn detect_returns_none_for_unknown() {
        assert_eq!(detect(Some(&PathBuf::from("foo.xyz")), None), None);
        assert_eq!(detect(None, None), None);
        assert_eq!(detect(None, Some("not a shebang")), None);
    }

    #[test]
    fn from_name_maps_common_aliases() {
        assert_eq!(Language::from_name("rust"), Some(Language::Rust));
        assert_eq!(Language::from_name("py"), Some(Language::Python));
        assert_eq!(Language::from_name("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_name("Rust"), Some(Language::Rust));
        assert_eq!(Language::from_name("unknown"), None);
    }

    #[test]
    fn rust_suppresses_single_quote_auto_pair() {
        let cfg = Language::Rust.config();
        assert!(cfg.auto_pair_suppress_quotes.contains(&'\''));
        assert!(!cfg.auto_pairs.iter().any(|(o, _)| *o == '\''));
    }

    #[test]
    fn html_family_auto_pairs_angle_brackets() {
        for lang in [Language::Html, Language::Xml, Language::Jsx, Language::Tsx] {
            assert!(
                lang.config().auto_pairs.iter().any(|(o, _)| *o == '<'),
                "expected angle-bracket auto-pair for {:?}",
                lang
            );
        }
        for lang in [Language::Rust, Language::TypeScript, Language::C] {
            assert!(
                !lang.config().auto_pairs.iter().any(|(o, _)| *o == '<'),
                "unexpected angle-bracket auto-pair for {:?}",
                lang
            );
        }
    }

    #[test]
    fn python_has_no_auto_dedent_closers() {
        assert!(Language::Python.config().auto_dedent_closers.is_empty());
    }

    #[test]
    fn c_family_auto_dedents_on_close_brace() {
        for lang in [
            Language::Rust,
            Language::JavaScript,
            Language::Java,
            Language::Go,
        ] {
            assert_eq!(lang.config().auto_dedent_closers, &['}']);
        }
    }

    #[test]
    fn indent_widths_match_common_conventions() {
        assert_eq!(Language::Rust.config().indent.width(), 4);
        assert_eq!(Language::Python.config().indent.width(), 4);
        assert_eq!(Language::JavaScript.config().indent.width(), 2);
        assert_eq!(Language::TypeScript.config().indent.width(), 2);
        assert_eq!(Language::Yaml.config().indent.width(), 2);
        assert!(Language::Go.config().indent.uses_tabs());
        assert!(Language::Makefile.config().indent.uses_tabs());
    }
}
