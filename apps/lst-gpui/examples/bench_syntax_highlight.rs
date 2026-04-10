use std::{
    env, fs,
    hint::black_box,
    path::PathBuf,
    process,
    sync::LazyLock,
    time::{Duration, Instant},
};

use syntect::{
    easy::HighlightLines, highlighting::Theme, parsing::SyntaxSet, util::LinesWithEndings,
};
use tree_sitter::{Language, Parser};
use tree_sitter_highlight::{
    HighlightConfiguration, HighlightEvent, Highlighter as TreeSitterHighlighter,
};

const RUST_CORPUS: &str = include_str!("../../../benchmarks/paste-corpus-20k.rs");
const TARGET_LINES: usize = 20_000;

const CAPTURE_NAMES: &[&str] = &[
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

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let data = include_bytes!("../../../src/catppuccin-mocha.tmTheme");
    let mut cursor = std::io::Cursor::new(&data[..]);
    syntect::highlighting::ThemeSet::load_from_reader(&mut cursor)
        .expect("embedded Catppuccin Mocha theme should be valid")
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Plain,
    TreeSitterParse,
    TreeSitterHighlight,
    Syntect,
}

impl Backend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::TreeSitterParse => "tree-sitter-parse",
            Self::TreeSitterHighlight => "tree-sitter-highlight",
            Self::Syntect => "syntect",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LanguageKind {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Toml,
    Yaml,
    Markdown,
    Html,
    Css,
}

impl LanguageKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Css => "css",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Css => "css",
        }
    }

    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Self::Markdown => tree_sitter_md::LANGUAGE.into(),
            Self::Html => tree_sitter_html::LANGUAGE.into(),
            Self::Css => tree_sitter_css::LANGUAGE.into(),
        }
    }

    fn tree_sitter_config(self) -> HighlightConfiguration {
        let (language, name, highlights, injections, locals) = match self {
            Self::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                "rust",
                tree_sitter_rust::HIGHLIGHTS_QUERY,
                tree_sitter_rust::INJECTIONS_QUERY,
                "",
            ),
            Self::Python => (
                tree_sitter_python::LANGUAGE.into(),
                "python",
                tree_sitter_python::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
            Self::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            ),
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "typescript",
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                "",
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            Self::Json => (
                tree_sitter_json::LANGUAGE.into(),
                "json",
                tree_sitter_json::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
            Self::Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                "toml",
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
            Self::Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                "yaml",
                tree_sitter_yaml::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
            Self::Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                "markdown",
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
                tree_sitter_md::INJECTION_QUERY_BLOCK,
                "",
            ),
            Self::Html => (
                tree_sitter_html::LANGUAGE.into(),
                "html",
                tree_sitter_html::HIGHLIGHTS_QUERY,
                tree_sitter_html::INJECTIONS_QUERY,
                "",
            ),
            Self::Css => (
                tree_sitter_css::LANGUAGE.into(),
                "css",
                tree_sitter_css::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
        };
        let mut config =
            HighlightConfiguration::new(language, name, highlights, injections, locals)
                .expect("embedded tree-sitter highlight query should be valid");
        config.configure(CAPTURE_NAMES);
        config
    }

    fn from_injection(language: &str) -> Option<Self> {
        match language.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" => Some(Self::Python),
            "javascript" | "js" | "jsx" => Some(Self::JavaScript),
            "typescript" | "ts" | "tsx" => Some(Self::TypeScript),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "markdown" | "md" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "css" => Some(Self::Css),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Args {
    iterations: usize,
    backend: Option<Backend>,
    language: Option<LanguageKind>,
    file: Option<PathBuf>,
    extension: Option<String>,
}

#[derive(Debug)]
struct Corpus {
    language: LanguageKind,
    extension: String,
    source: String,
}

#[derive(Debug)]
struct Measurement {
    duration: Duration,
    spans: usize,
    checksum: usize,
}

fn main() {
    let args = parse_args().unwrap_or_else(|error| {
        eprintln!("{error}");
        print_usage();
        process::exit(2);
    });

    if args.iterations == 0 {
        eprintln!("--iterations must be greater than zero");
        process::exit(2);
    }

    let corpora = load_corpora(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        process::exit(1);
    });
    let backends = selected_backends(args.backend);

    println!("backend\tlanguage\tlines\tbytes\titerations\tmedian_ms\tmin_ms\tspans\tchecksum");
    for corpus in &corpora {
        for backend in &backends {
            if !backend_supports_corpus(*backend, corpus) {
                continue;
            }
            let result = run_case(*backend, corpus, args.iterations).unwrap_or_else(|error| {
                eprintln!(
                    "{} {} failed: {error}",
                    backend.as_str(),
                    corpus.language.as_str()
                );
                process::exit(1);
            });
            println!(
                "{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{}\t{}",
                backend.as_str(),
                corpus.language.as_str(),
                line_count(&corpus.source),
                corpus.source.len(),
                args.iterations,
                millis(median_duration(&result)),
                millis(min_duration(&result)),
                result.last().map_or(0, |measurement| measurement.spans),
                result.last().map_or(0, |measurement| measurement.checksum)
            );
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        iterations: 5,
        backend: None,
        language: None,
        file: None,
        extension: None,
    };
    let mut raw = env::args().skip(1);

    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--iterations" => {
                let value = raw
                    .next()
                    .ok_or_else(|| "--iterations requires a value".to_string())?;
                args.iterations = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --iterations value: {value}"))?;
            }
            "--backend" => {
                let value = raw
                    .next()
                    .ok_or_else(|| "--backend requires a value".to_string())?;
                args.backend = match value.as_str() {
                    "all" => None,
                    _ => Some(parse_backend(&value)?),
                };
            }
            "--language" => {
                let value = raw
                    .next()
                    .ok_or_else(|| "--language requires a value".to_string())?;
                args.language = match value.as_str() {
                    "all" => None,
                    _ => Some(parse_language(&value)?),
                };
            }
            "--file" => {
                let value = raw
                    .next()
                    .ok_or_else(|| "--file requires a value".to_string())?;
                args.file = Some(PathBuf::from(value));
            }
            "--extension" => {
                args.extension = Some(
                    raw.next()
                        .ok_or_else(|| "--extension requires a value".to_string())?,
                );
            }
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }

    if args.file.is_some() && args.language.is_none() {
        return Err("--file requires --language so tree-sitter can choose a grammar".to_string());
    }

    Ok(args)
}

fn print_usage() {
    eprintln!(
        "usage: cargo run --release -p lst-gpui --example bench_syntax_highlight -- \\
         [--iterations N] [--backend all|plain|tree-sitter-parse|tree-sitter-highlight|syntect] \\
         [--language all|rust|python|javascript|typescript|json|toml|yaml|markdown|html|css] \\
         [--file PATH] [--extension EXT]"
    );
}

fn parse_backend(value: &str) -> Result<Backend, String> {
    match value {
        "plain" => Ok(Backend::Plain),
        "tree-sitter-parse" | "parse" => Ok(Backend::TreeSitterParse),
        "tree-sitter-highlight" | "tree-sitter" => Ok(Backend::TreeSitterHighlight),
        "syntect" => Ok(Backend::Syntect),
        _ => Err(format!("unknown backend: {value}")),
    }
}

fn parse_language(value: &str) -> Result<LanguageKind, String> {
    match value {
        "rust" | "rs" => Ok(LanguageKind::Rust),
        "python" | "py" => Ok(LanguageKind::Python),
        "javascript" | "js" => Ok(LanguageKind::JavaScript),
        "typescript" | "ts" => Ok(LanguageKind::TypeScript),
        "json" => Ok(LanguageKind::Json),
        "toml" => Ok(LanguageKind::Toml),
        "yaml" | "yml" => Ok(LanguageKind::Yaml),
        "markdown" | "md" => Ok(LanguageKind::Markdown),
        "html" | "htm" => Ok(LanguageKind::Html),
        "css" => Ok(LanguageKind::Css),
        _ => Err(format!("unknown language: {value}")),
    }
}

fn load_corpora(args: &Args) -> Result<Vec<Corpus>, String> {
    if let Some(path) = &args.file {
        let language = args
            .language
            .expect("--file requires --language; checked in parse_args");
        let extension = args
            .extension
            .clone()
            .or_else(|| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| language.extension().to_string());
        let source = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        return Ok(vec![Corpus {
            language,
            extension,
            source,
        }]);
    }

    let languages: Vec<LanguageKind> = match args.language {
        Some(language) => vec![language],
        None => vec![
            LanguageKind::Rust,
            LanguageKind::Python,
            LanguageKind::JavaScript,
            LanguageKind::TypeScript,
            LanguageKind::Json,
            LanguageKind::Toml,
            LanguageKind::Yaml,
            LanguageKind::Markdown,
            LanguageKind::Html,
            LanguageKind::Css,
        ],
    };

    Ok(languages
        .into_iter()
        .map(|language| Corpus {
            language,
            extension: language.extension().to_string(),
            source: builtin_corpus(language),
        })
        .collect())
}

fn selected_backends(backend: Option<Backend>) -> Vec<Backend> {
    match backend {
        Some(backend) => vec![backend],
        None => vec![
            Backend::Plain,
            Backend::TreeSitterParse,
            Backend::TreeSitterHighlight,
            Backend::Syntect,
        ],
    }
}

fn backend_supports_corpus(backend: Backend, corpus: &Corpus) -> bool {
    match backend {
        Backend::Plain => true,
        Backend::Syntect => SYNTAX_SET
            .find_syntax_by_extension(&corpus.extension)
            .is_some(),
        Backend::TreeSitterParse | Backend::TreeSitterHighlight => matches!(
            corpus.language,
            LanguageKind::Rust
                | LanguageKind::Python
                | LanguageKind::JavaScript
                | LanguageKind::TypeScript
                | LanguageKind::Json
                | LanguageKind::Toml
                | LanguageKind::Yaml
                | LanguageKind::Markdown
                | LanguageKind::Html
                | LanguageKind::Css
        ),
    }
}

fn run_case(
    backend: Backend,
    corpus: &Corpus,
    iterations: usize,
) -> Result<Vec<Measurement>, String> {
    let mut measurements = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let measurement = match backend {
            Backend::Plain => measure_plain(&corpus.source),
            Backend::TreeSitterParse => measure_tree_sitter_parse(corpus.language, &corpus.source)?,
            Backend::TreeSitterHighlight => {
                measure_tree_sitter_highlight(corpus.language, &corpus.source)?
            }
            Backend::Syntect => measure_syntect(&corpus.extension, &corpus.source)?,
        };
        black_box(measurement.checksum);
        measurements.push(measurement);
    }
    Ok(measurements)
}

fn measure_plain(source: &str) -> Measurement {
    let start = Instant::now();
    let mut checksum = 0usize;
    let mut spans = 0usize;
    for line in LinesWithEndings::from(source) {
        spans += 1;
        checksum = checksum.wrapping_add(line.len());
    }
    Measurement {
        duration: start.elapsed(),
        spans,
        checksum,
    }
}

fn measure_tree_sitter_parse(language: LanguageKind, source: &str) -> Result<Measurement, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.tree_sitter_language())
        .map_err(|error| format!("failed to set parser language: {error}"))?;

    let start = Instant::now();
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter parser returned no tree".to_string())?;
    let root = tree.root_node();
    let checksum = root
        .kind_id()
        .wrapping_add(root.start_byte() as u16)
        .wrapping_add(root.end_byte() as u16) as usize;
    let spans = root.named_child_count();
    Ok(Measurement {
        duration: start.elapsed(),
        spans,
        checksum,
    })
}

fn measure_tree_sitter_highlight(
    language: LanguageKind,
    source: &str,
) -> Result<Measurement, String> {
    let config = language.tree_sitter_config();
    let injection_configs = injection_configs();
    let mut highlighter = TreeSitterHighlighter::new();
    let start = Instant::now();
    let events = highlighter
        .highlight(&config, source.as_bytes(), None, |name| {
            let language = LanguageKind::from_injection(name)?;
            injection_configs
                .iter()
                .find(|(candidate, _)| *candidate == language)
                .map(|(_, config)| config)
        })
        .map_err(|error| format!("tree-sitter highlight failed: {error}"))?;

    let mut checksum = 0usize;
    let mut spans = 0usize;
    for event in events {
        match event.map_err(|error| format!("tree-sitter highlight event failed: {error}"))? {
            HighlightEvent::Source { start, end } if start < end => {
                spans += 1;
                checksum = checksum.wrapping_add(start).wrapping_add(end);
            }
            HighlightEvent::HighlightStart(highlight) => {
                checksum = checksum.wrapping_add(highlight.0);
            }
            HighlightEvent::HighlightEnd | HighlightEvent::Source { .. } => {}
        }
    }

    Ok(Measurement {
        duration: start.elapsed(),
        spans,
        checksum,
    })
}

fn injection_configs() -> Vec<(LanguageKind, HighlightConfiguration)> {
    [
        LanguageKind::Rust,
        LanguageKind::Python,
        LanguageKind::JavaScript,
        LanguageKind::TypeScript,
        LanguageKind::Json,
        LanguageKind::Toml,
        LanguageKind::Yaml,
        LanguageKind::Markdown,
        LanguageKind::Html,
        LanguageKind::Css,
    ]
    .into_iter()
    .map(|language| (language, language.tree_sitter_config()))
    .collect()
}

fn measure_syntect(extension: &str, source: &str) -> Result<Measurement, String> {
    let syntax = SYNTAX_SET
        .find_syntax_by_extension(extension)
        .ok_or_else(|| format!("syntect has no syntax for extension: {extension}"))?;
    let mut highlighter = HighlightLines::new(syntax, &THEME);

    let start = Instant::now();
    let mut checksum = 0usize;
    let mut spans = 0usize;
    for line in LinesWithEndings::from(source) {
        let ranges = highlighter
            .highlight_line(line, &SYNTAX_SET)
            .map_err(|error| format!("syntect highlight failed: {error}"))?;
        spans += ranges.len();
        for (style, range) in ranges {
            checksum = checksum
                .wrapping_add(range.len())
                .wrapping_add(style.foreground.r as usize)
                .wrapping_add(style.foreground.g as usize)
                .wrapping_add(style.foreground.b as usize);
        }
    }

    Ok(Measurement {
        duration: start.elapsed(),
        spans,
        checksum,
    })
}

fn median_duration(measurements: &[Measurement]) -> Duration {
    let mut durations: Vec<Duration> = measurements
        .iter()
        .map(|measurement| measurement.duration)
        .collect();
    durations.sort_unstable();
    durations[durations.len() / 2]
}

fn min_duration(measurements: &[Measurement]) -> Duration {
    measurements
        .iter()
        .map(|measurement| measurement.duration)
        .min()
        .expect("at least one measurement")
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn line_count(source: &str) -> usize {
    source.lines().count()
}

fn builtin_corpus(language: LanguageKind) -> String {
    match language {
        LanguageKind::Rust => RUST_CORPUS.to_string(),
        LanguageKind::Python => generated_python(),
        LanguageKind::JavaScript => generated_javascript(),
        LanguageKind::TypeScript => generated_typescript(),
        LanguageKind::Json => generated_json(),
        LanguageKind::Toml => generated_toml(),
        LanguageKind::Yaml => generated_yaml(),
        LanguageKind::Markdown => generated_markdown(),
        LanguageKind::Html => generated_html(),
        LanguageKind::Css => generated_css(),
    }
}

fn generated_python() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
from dataclasses import dataclass as dataclass_{i}

@dataclass_{i}
class Item{i}:
    name: str
    value: int

def compute_{i}(items: list[Item{i}]) -> dict[str, int]:
    total = 0
    result = {{}}
    for item in items:
        if item.value % 2 == 0:
            total += item.value
        else:
            total -= item.value
        result[item.name] = total
    return result

"
        ));
        i += 1;
    }
    source
}

fn generated_javascript() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
export class Item{i} {{
  constructor(name, value) {{
    this.name = name;
    this.value = value;
  }}

  compute(seed) {{
    const values = [this.value, seed, {i}];
    return values
      .filter((value) => value % 2 === 0)
      .map((value) => value * 3)
      .reduce((left, right) => left + right, 0);
  }}
}}

export function build{i}(items) {{
  return items.map((item) => new Item{i}(item.name, item.value));
}}

"
        ));
        i += 1;
    }
    source
}

fn generated_typescript() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
interface Item{i} {{
  name: string;
  value: number;
}}

export function compute{i}(items: Item{i}[]): Map<string, number> {{
  let total = 0;
  const result = new Map<string, number>();
  for (const item of items) {{
    total += item.value % 2 === 0 ? item.value : -item.value;
    result.set(item.name, total);
  }}
  return result;
}}

"
        ));
        i += 1;
    }
    source
}

fn generated_json() -> String {
    let mut source = String::from("[\n");
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
  {{
    \"id\": {i},
    \"name\": \"item-{i}\",
    \"enabled\": {},
    \"tags\": [\"editor\", \"highlight\", \"benchmark\"],
    \"metadata\": {{
      \"score\": {},
      \"ratio\": {:.3}
    }}
  }},
",
            i % 2 == 0,
            i * 17,
            i as f64 / 7.0
        ));
        i += 1;
    }
    source.push_str("  {\"id\": -1, \"name\": \"sentinel\"}\n]\n");
    source
}

fn generated_toml() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
[[items]]
id = {i}
name = \"item-{i}\"
enabled = {}
tags = [\"editor\", \"highlight\", \"benchmark\"]

",
            i % 2 == 0
        ));
        i += 1;
    }
    source
}

fn generated_yaml() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
- id: {i}
  name: item-{i}
  enabled: {}
  tags:
    - editor
    - highlight
    - benchmark

",
            i % 2 == 0
        ));
        i += 1;
    }
    source
}

fn generated_markdown() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
## Section {i}

This paragraph contains **strong text**, _emphasis_, and `inline_code_{i}`.

```rust
fn item_{i}() -> usize {{
    {i}
}}
```

"
        ));
        i += 1;
    }
    source
}

fn generated_html() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
<section class=\"item\" data-id=\"{i}\">
  <h2>Item {i}</h2>
  <style>
    .item-{i} {{ color: rgb({}, {}, {}); }}
  </style>
  <script>
    const item{i} = {{ id: {i}, enabled: {} }};
  </script>
</section>

",
            i % 255,
            (i * 2) % 255,
            (i * 3) % 255,
            i % 2 == 0
        ));
        i += 1;
    }
    source
}

fn generated_css() -> String {
    let mut source = String::new();
    let mut i = 0usize;
    while line_count(&source) < TARGET_LINES {
        source.push_str(&format!(
            "\
.item-{i} {{
  display: grid;
  grid-template-columns: 1fr auto;
  color: rgb({}, {}, {});
  --accent-{i}: #{:06x};
}}

",
            i % 255,
            (i * 2) % 255,
            (i * 3) % 255,
            (i * 9973) % 0x00ff_ffff
        ));
        i += 1;
    }
    source
}
