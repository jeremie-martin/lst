# Highlight Attribution Notes

Date of these measurements: `2026-04-09`

Note: the paste benchmark now uses `benchmarks/paste-corpus-20k.rs` (~21.5k lines).
The current contract copies from one tab into a second empty tab and waits until the saved target file exactly matches the corpus before finishing.
The concrete scores below were collected on the older smaller paste corpus and should
be treated as historical comparison notes, not current benchmark targets.

Workload:

- benchmark: `bench_paste_x11`
- corpus: `benchmarks/paste-corpus.rs`
- real X11 display
- real injected GUI input
- pure-append 5-second paste trace
- score: median `cpu_ms`

These runs were used to answer two questions:

1. Is the benchmark measuring real editor work?
2. If highlighting is expensive, is the cost mostly parsing or mostly drawing colored text?

## Sanity checks

Representative scores from the same benchmark contract on this machine:

- default highlighting: about `3550` to `3910`
- highlighting disabled entirely: about `1490`
- fake cheap colored spans with no real parsing: about `1400`
- current parser work but emit no spans: about `3500`
- wrap disabled: about `3760`
- gutter blanked: about `3820`
- highlight off + wrap off + gutter blank: about `1090`

Interpretation:

- the benchmark is responsive to real application work
- it is not dominated by fixed trace timing, compositor overhead, or benchmark harness cost
- wrapping and gutter text are not the main cost in this paste scenario
- colored text drawing by itself is relatively cheap here
- the expensive layer is the current syntax engine, especially its parse/style pipeline

## Rust Backend Comparison

The current default Rust backend is the lightweight line-based tree-sitter path.

To force the old Rust `syntect` path for comparison:

```bash
LST_HIGHLIGHT_BACKEND=syntect
```

Current behavior of the default Rust backend:

- only affects Rust files
- line-based highlighting, not full-document incremental highlighting
- intended to stay lightweight and benchmark-friendly

Representative benchmark comparison:

- current default tree-sitter path: `score=2180`
- repeat tree-sitter run: about `2180`
- `syntect` fallback path: `score=3910`

Interpretation:

- the alternate backend is materially faster on the current paste benchmark
- the gain is large enough to justify making it the current default for Rust files
- the current implementation is deliberately simple and may still have highlighting-quality gaps on multiline constructs, so the `syntect` fallback remains useful for comparison

## Practical takeaway

The benchmark is good enough to guide highlighting work.

For the current paste scenario:

- optimizing highlighting is justified
- replacing or simplifying the Rust syntax engine was a credible direction and paid off on this workload
- GUI rendering is not the main bottleneck

## Broad-language Backend Characterization

Date of these measurements: `2026-04-10`

Use the GPUI highlighter benchmark for broad-language backend comparisons:

```bash
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --iterations 5
```

This benchmark highlights whole documents of roughly 20k lines and reports
`median_ms`. The `tree-sitter-highlight` rows use the production tree-sitter
pipeline, including conversion to per-line `SyntaxSpan`s. It is intentionally
colder and broader than the editor's visible viewport path, so it should guide
backend selection rather than represent a per-keystroke UI budget.

Representative results on this machine:

```text
backend                 language    lines  median_ms
tree-sitter-highlight   rust        21558  166.618
syntect                 rust        21558  1497.704
tree-sitter-highlight   python      20016  82.786
syntect                 python      20016  1249.067
tree-sitter-highlight   javascript  20007  135.911
syntect                 javascript  20007  1395.146
tree-sitter-highlight   typescript  20010  79.840
tree-sitter-highlight   json        20003  43.216
syntect                 json        20003  306.965
tree-sitter-highlight   toml        20004  61.075
tree-sitter-highlight   yaml        20000  51.022
syntect                 yaml        20000  217.756
tree-sitter-highlight   markdown    20000  164.546
syntect                 markdown    20000  2077.389
tree-sitter-highlight   html        20000  91.831
syntect                 html        20000  954.472
tree-sitter-highlight   css         20006  55.049
syntect                 css         20006  687.104
```

Interpretation:

- `syntect` is broad, but the measured cost is too high for the editor's
  default synchronous highlighting path on large files.
- `syntect` rows are omitted when the default syntax set has no syntax for that
  extension, because plain-text fallback timings are not valid highlighting
  measurements.
- `tree-sitter-highlight` is consistently faster here and should be the first
  production path for broad language support.
- The remaining performance work is architectural: background computation,
  revision-keyed caches, and eventually incremental or visible-range updates.
