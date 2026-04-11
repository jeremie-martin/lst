# Performance Optimization Workflow

This repository now has concrete performance workflows for both the shipping
`iced` editor and the GPUI rewrite.

The goal is simple:

- preserve behavior
- run one fixed benchmark scenario
- optimize one scalar metric that matches the user-visible problem
- use the other printed values as diagnostics

## GPUI performance benchmarks

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

### GPUI editor scenarios

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

## Recommended next benchmark

Use the **paste benchmark** when the problem is large copy/paste latency.
That is the current recommendation for this repo's paste-lag investigation,
because it measures one real large copy plus one real large paste into an empty
target tab and does not finish until the pasted file is actually complete.

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
```

Use the editing benchmark only when the question is broader overall editor
throughput rather than the single large-paste freeze.

## Which value to optimize

For `bench_paste_x11`, optimize:

```text
trace_wall_ms
```

The current contract is:

```text
trace_wall_ms = median(end-to-end elapsed time over the 7 measured repetitions)
score = median(cpu_ms over the same 7 measured repetitions)
cpu_ms = user_cpu_ms + sys_cpu_ms
```

Lower is better. For this paste benchmark, the optimization loop should minimize
`trace_wall_ms`, because that is the closest scalar to the user's visible wait:
the time from starting the copy/paste trace until the target file has the full
pasted contents and is stable on disk.

`score` is still useful, but as a secondary diagnostic. It tells you how much
CPU time the editor process consumed while the paste was happening.

## Paste benchmark (single-phase)

A single-phase benchmark that exercises a real large copy and a completed large paste on a prebuilt Rust corpus.
Useful when the question is "how bad is large copy/paste latency?" rather than general editing throughput.

Scenario:

- real-display X11 benchmark
- real injected keyboard input via XTEST
- file: `benchmarks/paste-corpus-20k.rs` (frozen Rust corpus, ~21558 lines)
- wrap: on
- highlighting: default Rust tree-sitter highlighting
- measured trace: focus editor, `Ctrl+A`, `Ctrl+C`, wait until the X11 clipboard matches the corpus size, `Ctrl+Tab` into a second empty tab, `Ctrl+V` once, then keep retrying `Ctrl+S` until the target file exactly matches the corpus size and stays stable
- `1s` sleep between repetitions
- `1` priming run, `7` measured runs
- initial file size: `801012` bytes, `21558` lines
- expected final target file size on every measured run: `801012` bytes, `21558` lines

Runner:

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
```

## Other printed values

The runner also prints diagnostics:

- `startup_ms`
- `select_all_ms`
- `copy_clipboard_ms`
- `tab_switch_ms`
- `paste_complete_ms`
- `paste_push_undo_ms`
- `paste_perform_ms`
- `paste_mark_changed_ms`
- `paste_update_total_ms`
- `trace_wall_ms`
- `user_cpu_ms`
- `sys_cpu_ms`
- `cpu_ms`
- `trace_damage_events`
- `paste_damage_events`
- `save_retry_count`
- `damage_hz_proxy`
- `peak_rss_mb`
- `final_file_bytes`
- `final_file_lines`

Use them for interpretation, not as the optimization target.

In particular:

- `copy_clipboard_ms` is the clipboard propagation diagnostic
- `paste_complete_ms` is the paste-only portion of the trace after `Ctrl+V`
- `paste_perform_ms` is the internal editor-model insertion time recorded around `iced::text_editor::Content::perform(Edit::Paste)`
- `paste_mark_changed_ms` is the app-owned post-paste bookkeeping time after the insertion
- `paste_update_total_ms` is the total duration of the app's paste update handler
- `save_retry_count` tells you how many `Ctrl+S` retries were needed before the target file matched
- `damage_hz_proxy` is an XDamage redraw-cadence proxy for responsiveness, not literal display FPS
- `peak_rss_mb` is the memory diagnostic
- `startup_ms` is useful context but not part of this campaign
- `final_file_bytes` and `final_file_lines` should match the benchmark's printed `expected_final_file_bytes` and `expected_final_file_lines`
- `trace_wall_ms` is the primary optimization target for this benchmark
- `score` remains median editor CPU time and is a useful secondary signal
- `damage_hz_proxy` is often directionally useful, but it is not the optimization target

Recent attribution notes for the paste benchmark are in [docs/highlight-attribution.md](/home/jmartin/lst/docs/highlight-attribution.md).
Those notes include the syntax-highlighting sanity checks and the current Rust backend comparison.
The framework-level assessment of `iced 0.14` and its large-paste path is in [docs/iced-text-editor-assessment.md](/home/jmartin/lst/docs/iced-text-editor-assessment.md).

## Characterization Mode

When the goal is attribution rather than optimization, the paste benchmark now
supports a few runtime-only ablations. These do not change the default
benchmark contract; they exist to answer "where is the time going?"

Use:

```bash
LST_BENCH_DISABLE_HIGHLIGHT=1 ./target/release/bench_paste_x11
LST_BENCH_DISABLE_GUTTER=1 ./target/release/bench_paste_x11
LST_BENCH_FORCE_NOWRAP=1 ./target/release/bench_paste_x11
```

You can combine them when you want a stripped-down floor estimate:

```bash
LST_BENCH_DISABLE_HIGHLIGHT=1 \
LST_BENCH_DISABLE_GUTTER=1 \
LST_BENCH_FORCE_NOWRAP=1 \
./target/release/bench_paste_x11
```

Read these as attribution experiments, not as the default optimization target.
The production optimization loop should still run the default benchmark and
optimize `trace_wall_ms`.

## Behavior-preservation gate

The blind refactor gate is:

```bash
cargo test
```

The default rule is: do not trust a performance change unless `cargo test` stays green.

## Intended edit scope

Production optimization work for the shipping `iced` editor should primarily
touch files under `src/`. Production optimization work for GPUI should primarily
touch files under `apps/lst-gpui/src/` and shared behavior crates under
`crates/`.

It is also acceptable to edit:

- `src/bin/bench_scroll_x11.rs` if the benchmark itself needs refinement
- `src/bin/bench_paste_x11.rs` if the benchmark itself needs refinement
- `src/bin/bench_editing_x11.rs` if the benchmark itself needs refinement
- `apps/lst-gpui/examples/bench_editor_x11.rs` if the GPUI benchmark itself needs refinement
- `README.md`
- `docs/`

Do not broaden the project into a generalized benchmark framework. Keep the workflow narrow and simple.

## GPUI syntax highlighting characterization

Use the GPUI syntax-highlighting benchmark when evaluating broad-language
highlighting backends. This is a full-document cold workload, not an editor
interaction benchmark. It exists to compare candidate highlighter engines under
the same input size before wiring them into the editor.

Runner:

```bash
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --iterations 5
```

Primary value:

```text
median_ms
```

Lower is better. Compare rows with the same `language` and `lines`; do not
compare `median_ms` across different corpus sizes as a product-level score.

The benchmark currently prints TSV columns:

```text
backend language lines bytes iterations median_ms min_ms spans checksum
```

Backends:

- `plain`: line-iteration floor; not a syntax-highlighting backend
- `tree-sitter-parse`: parse-only lower bound for grammar-based highlighting
- `tree-sitter-highlight`: production tree-sitter highlighting plus per-line
  `SyntaxSpan` conversion
- `syntect`: broad TextMate/sublime-syntax highlighting baseline

Representative results collected on `2026-04-10` on this machine:

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

- `syntect` remains useful as a broad-coverage baseline, but it is too slow to
  be the default synchronous path for large documents.
- `syntect` rows are omitted when the default syntax set has no syntax for that
  extension, because plain-text fallback timings are not valid highlighting
  measurements.
- Tree-sitter highlighting is the current preferred direction for broad
  language support because it is consistently much faster on this workload.
- Production editor integration should not run full-document highlighting on
  the UI path for every edit. Use background work, cache by document revision,
  and move toward incremental or visible-range updates where possible.

## Editing benchmark (comprehensive)

A multi-phase benchmark that exercises paste, scroll, find, and vim in a single run.
Better for overall optimization work because it covers more code paths and has tighter
variance (~3% spread vs ~30% for paste-only).

When using the editing benchmark instead of the paste benchmark, optimize its
final `score=...` line as before.

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
2. For `bench_paste_x11`, look at `trace_wall_ms` first.
3. Prefer lower `trace_wall_ms`.
4. Check that `final_file_bytes` and `final_file_lines` still match `expected_final_file_bytes` and `expected_final_file_lines`.
5. Check `score`, `cpu_ms`, and `peak_rss_mb` for obvious regressions.
6. Optionally inspect `damage_hz_proxy` as a redraw-responsiveness hint.
7. Run `cargo test`.

If you are using `bench_editing_x11` instead, use its final `score=...` line as
the primary scalar.

## Rust Highlight Comparison

To compare the current default Rust highlighter with the `syntect` fallback:

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
LST_HIGHLIGHT_BACKEND=syntect ./target/release/bench_paste_x11
```

The default contract now uses the Rust tree-sitter backend. Use the `syntect` fallback run for comparison and regression checks.
