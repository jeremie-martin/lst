# Performance Optimization Workflow

This repository now has three concrete performance workflows.

The goal is simple:

- preserve behavior
- run one fixed benchmark scenario
- optimize one scalar score
- use the other printed values as diagnostics

## Recommended next benchmark

Use this benchmark when the concern is unnecessary CPU during real editing work, not just scroll smoothness.

Scenario:

- real-display X11 benchmark
- real injected keyboard input via XTEST
- file: `benchmarks/paste-corpus.rs`
- wrap: on
- highlighting: default Rust tree-sitter highlighting
- setup: focus editor, `Ctrl+A`, `Ctrl+C`, `Ctrl+End`
- visible 5-second paste trace: `10` `Ctrl+V` pastes at `500ms` intervals
- `1s` sleep between repetitions
- `1` priming run, `7` measured runs
- expected final file size on every measured run: `1468522` bytes, `39523` lines

Runner:

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
```

## Which value to optimize

Use the final line:

```text
score=...
```

The current contract is:

```text
score = median(cpu_ms over the 7 measured repetitions)
cpu_ms = user_cpu_ms + sys_cpu_ms
```

Lower is better. The optimization loop should minimize `score`.

This is the right current score because the main concern is unnecessary CPU during real editing work on a growing buffer. The benchmark uses real display, real injected GUI input, and fixed repeated paste work.

## Other printed values

The runner also prints diagnostics:

- `startup_ms`
- `trace_wall_ms`
- `user_cpu_ms`
- `sys_cpu_ms`
- `cpu_ms`
- `peak_rss_mb`
- `final_file_bytes`
- `final_file_lines`

Use them for interpretation, not as the optimization target.

In particular:

- `peak_rss_mb` is the memory diagnostic
- `startup_ms` is useful context but not part of this campaign
- `final_file_bytes` and `final_file_lines` confirm that every run completed the same fixed paste workload

Recent attribution notes for the paste benchmark are in [docs/highlight-attribution.md](/home/jmartin/lst/docs/highlight-attribution.md).
Those notes include the syntax-highlighting sanity checks and the current Rust backend comparison.

## Behavior-preservation gate

The blind refactor gate is:

```bash
cargo test
```

The default rule is: do not trust a performance change unless `cargo test` stays green.

## Intended edit scope

Production optimization work should primarily touch files under `src/`.

It is also acceptable to edit:

- `src/bin/bench_scroll_x11.rs` if the benchmark itself needs refinement
- `src/bin/bench_paste_x11.rs` if the benchmark itself needs refinement
- `src/bin/bench_editing_x11.rs` if the benchmark itself needs refinement
- `README.md`
- `docs/`

Do not broaden the project into a generalized benchmark framework. Keep the workflow narrow and simple.

## Editing benchmark (comprehensive)

A multi-phase benchmark that exercises paste, scroll, find, and vim in a single run.
Better for overall optimization work because it covers more code paths and has tighter
variance (~3% spread vs ~30% for paste-only).

Scenario:

- real-display X11 benchmark
- real injected keyboard input via XTEST
- file: `benchmarks/editing-corpus.rs` (frozen copy of app.rs, ~3593 lines)
- wrap: on
- highlighting: default Rust tree-sitter highlighting
- 5 phases in one CPU-measurement window:
  1. **Paste growth** (~5s): Ctrl+A, Ctrl+C, Ctrl+End, then 10 Ctrl+V at 500ms intervals
  2. **Scroll** (~3s): 240 wheel-down over 1.5s, 240 wheel-up over 1.5s
  3. **Find cycling** (~5s): Ctrl+F, type each letter a→z (with backspace), then "fn " + 30 Enter navigations
  4. **Vim navigation** (~3s): Escape, 10 cycles of gg (top) and G (bottom)
  5. **Vim yank+paste** (~3s): gg, 500yy, G, 10p (adds 5000 lines)
- `1s` sleep between repetitions
- `1` priming run, `7` measured runs
- expected final file size on every measured run: `1468664` bytes, `39552` lines

Runner:

```bash
cargo build --release --bin lst --bin bench_editing_x11
./target/release/bench_editing_x11
```

## Scroll benchmark

The original scroll benchmark is still available as a separate scenario:

```bash
cargo build --release --bin lst --bin bench_scroll_x11
./target/release/bench_scroll_x11
```

Use it when the question is scrolling cost specifically. Keep its `score=...` line separate from the paste benchmark. Do not combine both workloads into one scalar.

## Practical reading of results

When evaluating a change:

1. Run the benchmark.
2. Look at the final `score=...` line.
3. Prefer lower `score`.
4. Check that `final_file_bytes` and `final_file_lines` still match the fixed contract.
5. Check `peak_rss_mb` for obvious regressions.
6. Run `cargo test`.

That is the current optimization contract.

## Rust Highlight Comparison

To compare the current default Rust highlighter with the `syntect` fallback:

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
LST_HIGHLIGHT_BACKEND=syntect ./target/release/bench_paste_x11
```

The default contract now uses the Rust tree-sitter backend. Use the `syntect` fallback run for comparison and regression checks.
