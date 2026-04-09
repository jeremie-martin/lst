# Highlight Attribution Notes

Date of these measurements: `2026-04-09`

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

## Tree-sitter spike

An experimental Rust-only alternate backend was added behind:

```bash
LST_HIGHLIGHT_BACKEND=tree-sitter
```

Current behavior of the spike:

- only affects Rust files
- line-based highlighting, not full-document incremental highlighting
- intended for comparison and iteration, not yet as a final design claim

Representative benchmark comparison:

- default `syntect` path: `score=3910`
- experimental `tree-sitter` path: `score=2190`
- repeat experimental `tree-sitter` run: `score=2180`

Interpretation:

- the alternate backend is materially faster on the current paste benchmark
- the gain is large enough to justify keeping the experiment for further evaluation
- this does not yet prove it should become the default, because the current spike is deliberately simple and may have highlighting-quality gaps on multiline constructs

## Practical takeaway

The benchmark is good enough to guide highlighting work.

For the current paste scenario:

- optimizing highlighting is justified
- replacing or simplifying the syntax engine is a credible direction
- GUI rendering is not the main bottleneck
