# Performance Optimization Workflow

This document is for the active GPUI editor. The archived iced editor and its
old benchmark harness live under `legacy/iced-lst` and are not part of the
active optimization workflow.

The goal is simple:

- preserve behavior
- run one fixed benchmark scenario
- optimize one scalar metric that matches the user-visible problem
- use the other printed values as diagnostics

## GPUI Benchmarks

The GPUI editor has a real-display X11 benchmark runner for editor interaction
latency and a separate syntax-highlighting benchmark for the production
tree-sitter path.

Build the GPUI editor and both benchmark examples:

```bash
cargo build --release -p lst-gpui --bin lst-gpui --example bench_editor_x11 --example bench_syntax_highlight
```

Run every GPUI editor interaction scenario:

```bash
./target/release/examples/bench_editor_x11 --scenario all
```

Run one scenario when optimizing a specific path:

```bash
./target/release/examples/bench_editor_x11 --scenario large-paste
./target/release/examples/bench_editor_x11 --scenario typing-large
./target/release/examples/bench_editor_x11 --scenario scroll-highlighted
./target/release/examples/bench_editor_x11 --scenario search-large
```

Run the syntax-highlighting benchmark:

```bash
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --backend tree-sitter-highlight --language rust --iterations 7
```

The editor interaction runner requires a real X11 desktop session with XTEST
and XDamage. It may discover `DISPLAY`/`XAUTHORITY` from another desktop
process, but the most reliable mode is running it from a desktop terminal.
`Xvfb` is not representative for this GPUI path on this host because GPUI
surface creation needs a real presentation backend.

## Scenarios

Each scenario prints `primary_metric`, `primary_value`, per-run values, and
secondary diagnostics such as CPU time, damage events, peak RSS, and final file
size where relevant.

| Scenario | Primary metric | Completion condition |
| --- | --- | --- |
| `large-paste` | `paste_complete_ms` | Copies the large Rust corpus, pastes into a second file tab, then retries `Ctrl+S` until the target file exactly matches the corpus and stays stable. |
| `typing-medium` | `typing_ms_per_char` | Types a fixed lowercase payload into `benchmarks/editing-corpus.rs`, waits for redraw quiet, then verifies the saved file exactly matches the expected text. |
| `typing-large` | `typing_ms_per_char` | Same as `typing-medium`, using `benchmarks/paste-corpus-20k.rs`. |
| `scroll-highlighted` | `scroll_overrun_ms` | Scrolls down and back through the large Rust file on a fixed input schedule, then waits for redraw quiet. |
| `scroll-plain` | `scroll_overrun_ms` | Same scroll trace using the large corpus as `.txt`, so syntax highlighting is out of the path. |
| `open-large` | `open_to_quiet_ms` | Measures process spawn through benchmark window discovery and redraw quiet on the large Rust file. |
| `search-large` | `search_reindex_ms` | Opens find, types `fn `, waits for redraw quiet, and reads the completed in-app find reindex trace. |

The default runner contract is `1` priming run and `7` measured repetitions.
Use `--repetitions <n>` and `--priming <n>` only when characterizing variance
or shortening a local smoke test.

The GPUI app writes internal benchmark trace values only when
`LST_BENCH_TRACE_FILE` is set by the runner. Normal editor runs do not create
trace files.

## Current Baseline

Current baseline collected on 2026-04-11 on `DISPLAY=:1`:

| Scenario | Primary metric | Median |
| --- | --- | ---: |
| `large-paste` | `paste_complete_ms` | `226.909 ms` |
| `typing-medium` smoke | `typing_ms_per_char` | `0.276 ms` |
| `scroll-highlighted` | `scroll_overrun_ms` | `1087.519 ms` |
| `scroll-plain` | `scroll_overrun_ms` | `1085.850 ms` |
| `open-large` | `open_to_quiet_ms` | `1153.185 ms` |
| syntax highlight rust | `median_ms` | `176.425 ms` |

Known benchmark gaps:

- `search-large` is currently invalid: the runner opens find and focus is
  requested, but injected text still reaches the editor instead of the find
  input.
- `typing-large` still needs a full default run after the input-positioning
  fix; `typing-medium` currently has a one-repetition smoke result.

## Behavior Gate

The active refactor gate is:

```bash
cargo test
```

For deeper Vim state-machine coverage:

```bash
cargo test -p lst-editor --features internal-invariants
```

Do not trust a performance change unless the active test gate stays green.

## Intended Edit Scope

Production optimization work should primarily touch:

- `apps/lst-gpui/src`
- `apps/lst-gpui/examples`
- `crates/lst-core`
- `crates/lst-editor`
- `crates/lst-ui`
- `benchmarks` when benchmark fixtures need deliberate changes
- `docs` and `README.md` when benchmark contracts change

Do not broaden the project into a generalized benchmark framework. Keep the
workflow narrow and simple.

## Syntax Highlighting

Use the GPUI syntax-highlighting benchmark when evaluating broad-language
highlighting backends. This is a full-document cold workload, not an editor
interaction benchmark. It exists to compare candidate highlighter engines under
the same input size before wiring them into the editor.

To compare the current default Rust highlighter with the `syntect` fallback:

```bash
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --backend tree-sitter-highlight --language rust --iterations 7
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --backend syntect --language rust --iterations 7
```

Recent attribution notes are in `docs/highlight-attribution.md`.
