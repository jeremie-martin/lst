use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[rustfmt::skip]
pub enum Language {
    Rust, Python, JavaScript, Jsx, TypeScript, Tsx, Json, Jsonc, Toml, Yaml, Markdown, Html, Xml,
    Css, Scss, C, Cpp, Java, Go, CSharp, Swift, Kotlin, Scala, Zig, Shell, Bash, Zsh, Fish, Ruby,
    Perl, Lua, Sql, Haskell, Elixir, Erlang, Clojure, CommonLisp, Scheme, EmacsLisp, Dockerfile,
    Makefile, CMake, Ini, Proto, Vim, Tex,
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

const PAIRS_BASIC: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const PAIRS_NO_SQ: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];
const PAIRS_ANGLE: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
    ('<', '>'),
];

const CL_BR: &[char] = &['}'];
const CL_NO: &[char] = &[];
const SUP_SQ: &[char] = &['\''];
const SUP_NO: &[char] = &[];

const fn lc(
    indent: IndentStyle,
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    auto_pairs: &'static [(char, char)],
    auto_pair_suppress_quotes: &'static [char],
    auto_dedent_closers: &'static [char],
) -> LanguageConfig {
    LanguageConfig {
        indent,
        line_comment,
        block_comment,
        auto_pairs,
        auto_pair_suppress_quotes,
        auto_dedent_closers,
    }
}

const fn sp(width: usize) -> IndentStyle {
    IndentStyle::Spaces { width }
}
const fn tb(width: usize) -> IndentStyle {
    IndentStyle::Tabs {
        display_width: width,
    }
}

#[rustfmt::skip]
const CONFIGS: &[LanguageConfig] = &[
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_NO_SQ, SUP_SQ, CL_BR), // Rust
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Python
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // JavaScript
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_ANGLE, SUP_NO, CL_BR), // Jsx
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // TypeScript
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_ANGLE, SUP_NO, CL_BR), // Tsx
    lc(sp(2), None,       None,                     PAIRS_BASIC, SUP_NO, CL_BR), // Json
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Jsonc
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Toml
    lc(sp(2), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Yaml
    lc(sp(2), None,       BLOCK_HTML,               PAIRS_BASIC, SUP_NO, CL_NO), // Markdown
    lc(sp(2), None,       BLOCK_HTML,               PAIRS_ANGLE, SUP_NO, CL_NO), // Html
    lc(sp(2), None,       BLOCK_HTML,               PAIRS_ANGLE, SUP_NO, CL_NO), // Xml
    lc(sp(2), None,       BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Css
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Scss
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // C
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Cpp
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Java
    lc(tb(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Go
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // CSharp
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Swift
    lc(sp(4), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Kotlin
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Scala
    lc(sp(4), Some("//"), None,                     PAIRS_BASIC, SUP_NO, CL_BR), // Zig
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Shell
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Bash
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Zsh
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Fish
    lc(sp(2), Some("#"),  Some(("=begin", "=end")), PAIRS_BASIC, SUP_NO, CL_NO), // Ruby
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_BR), // Perl
    lc(sp(4), Some("--"), Some(("--[[", "]]")),     PAIRS_BASIC, SUP_NO, CL_NO), // Lua
    lc(sp(2), Some("--"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_NO), // Sql
    lc(sp(2), Some("--"), Some(("{-", "-}")),       PAIRS_BASIC, SUP_NO, CL_NO), // Haskell
    lc(sp(2), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Elixir
    lc(sp(4), Some("%"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Erlang
    lc(sp(2), Some(";;"), None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Clojure
    lc(sp(2), Some(";;"), Some(("#|", "|#")),       PAIRS_BASIC, SUP_NO, CL_NO), // CommonLisp
    lc(sp(2), Some(";;"), Some(("#|", "|#")),       PAIRS_BASIC, SUP_NO, CL_NO), // Scheme
    lc(sp(2), Some(";;"), None,                     PAIRS_BASIC, SUP_NO, CL_NO), // EmacsLisp
    lc(sp(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Dockerfile
    lc(tb(4), Some("#"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Makefile
    lc(sp(2), Some("#"),  Some(("#[[", "]]")),      PAIRS_BASIC, SUP_NO, CL_NO), // CMake
    lc(sp(4), Some(";"),  None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Ini
    lc(sp(2), Some("//"), BLOCK_C,                  PAIRS_BASIC, SUP_NO, CL_BR), // Proto
    lc(sp(2), Some("\""), None,                     PAIRS_BASIC, SUP_NO, CL_NO), // Vim
    lc(sp(2), Some("%"),  None,                     PAIRS_BASIC, SUP_NO, CL_BR), // Tex
];

impl Language {
    pub fn config(self) -> &'static LanguageConfig {
        &CONFIGS[self as usize]
    }

    #[rustfmt::skip]
    fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Self::Rust,
            "python" | "py" => Self::Python,
            "javascript" | "js" => Self::JavaScript,
            "jsx" => Self::Jsx,
            "typescript" | "ts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "json" => Self::Json,
            "jsonc" | "json5" => Self::Jsonc,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "markdown" | "md" => Self::Markdown,
            "html" | "htm" => Self::Html,
            "xml" => Self::Xml,
            "css" => Self::Css,
            "scss" => Self::Scss,
            "c" => Self::C,
            "cpp" | "c++" | "cc" | "cxx" => Self::Cpp,
            "java" => Self::Java,
            "go" | "golang" => Self::Go,
            "c_sharp" | "csharp" | "c#" | "cs" => Self::CSharp,
            "swift" => Self::Swift,
            "kotlin" | "kt" => Self::Kotlin,
            "scala" => Self::Scala,
            "zig" => Self::Zig,
            "shell" | "sh" => Self::Shell,
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            "ruby" | "rb" => Self::Ruby,
            "perl" | "pl" => Self::Perl,
            "lua" => Self::Lua,
            "sql" => Self::Sql,
            "haskell" | "hs" => Self::Haskell,
            "elixir" | "ex" | "exs" => Self::Elixir,
            "erlang" | "erl" => Self::Erlang,
            "clojure" | "clj" | "cljs" => Self::Clojure,
            "common_lisp" | "commonlisp" | "lisp" | "cl" => Self::CommonLisp,
            "scheme" | "scm" | "racket" | "rkt" => Self::Scheme,
            "emacs_lisp" | "emacslisp" | "elisp" | "el" => Self::EmacsLisp,
            "dockerfile" => Self::Dockerfile,
            "makefile" | "make" => Self::Makefile,
            "cmake" => Self::CMake,
            "ini" | "conf" | "cfg" => Self::Ini,
            "proto" | "protobuf" => Self::Proto,
            "vim" | "vimscript" => Self::Vim,
            "tex" | "latex" => Self::Tex,
            _ => return None,
        })
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

#[rustfmt::skip]
fn detect_from_filename(name: &str) -> Option<Language> {
    Some(match name {
        "Makefile" | "makefile" | "GNUmakefile" | "BSDmakefile" => Language::Makefile,
        "Dockerfile" | "dockerfile" | "Containerfile" => Language::Dockerfile,
        "CMakeLists.txt" => Language::CMake,
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
        "xml" | "xhtml" | "svg" => Language::Xml,
        "css" => Language::Css,
        "scss" | "sass" => Language::Scss,
        "c" | "h" => Language::C,
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "c++" | "h++" => Language::Cpp,
        "java" => Language::Java,
        "go" => Language::Go,
        "cs" => Language::CSharp,
        "swift" => Language::Swift,
        "kt" | "kts" => Language::Kotlin,
        "scala" | "sc" => Language::Scala,
        "zig" => Language::Zig,
        "sh" => Language::Shell,
        "bash" => Language::Bash,
        "zsh" => Language::Zsh,
        "fish" => Language::Fish,
        "rb" | "ruby" => Language::Ruby,
        "pl" | "pm" => Language::Perl,
        "lua" => Language::Lua,
        "sql" => Language::Sql,
        "hs" | "lhs" => Language::Haskell,
        "ex" | "exs" => Language::Elixir,
        "erl" | "hrl" => Language::Erlang,
        "clj" | "cljs" | "cljc" | "edn" => Language::Clojure,
        "lisp" | "cl" | "asd" => Language::CommonLisp,
        "scm" | "ss" | "rkt" => Language::Scheme,
        "el" => Language::EmacsLisp,
        "dockerfile" => Language::Dockerfile,
        "mk" => Language::Makefile,
        "cmake" => Language::CMake,
        "ini" | "conf" | "cfg" | "properties" => Language::Ini,
        "proto" => Language::Proto,
        "vim" | "vimrc" => Language::Vim,
        "tex" | "latex" | "sty" | "cls" => Language::Tex,
        _ => return None,
    })
}

fn detect_from_shebang(first_line: &str) -> Option<Language> {
    let rest = first_line.strip_prefix("#!")?.trim_start();
    let (head, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let interpreter = if head.ends_with("/env") || head == "env" {
        tail.trim_start().split_whitespace().next().unwrap_or("")
    } else {
        head.rsplit('/').next().unwrap_or("")
    };
    let interpreter = interpreter.trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '.');
    Language::from_name(interpreter).or(match interpreter {
        "node" => Some(Language::JavaScript),
        _ => None,
    })
}

const CONFIG_DEFAULT: LanguageConfig = lc(sp(4), None, None, PAIRS_BASIC, SUP_NO, CL_BR);

pub(crate) const DEFAULT_CONFIG: &LanguageConfig = &CONFIG_DEFAULT;

pub(crate) fn config_for(lang: Option<Language>) -> &'static LanguageConfig {
    match lang {
        Some(l) => l.config(),
        None => DEFAULT_CONFIG,
    }
}
