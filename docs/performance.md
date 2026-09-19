# Performance

Measure a user-visible problem with one fixed scenario and one primary metric.
Preserve behavior, compare the same corpus and display environment, and use the
remaining metrics only to explain the result.

## GPUI/X11 benchmark

The end-to-end runner launches the release `lst` binary, sends XTEST input,
observes XDamage and application trace events, and verifies saved output for
editing scenarios.

Build the app and runner together:

```sh
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
```

Run a quick pass or a normal baseline:

```sh
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario all --repetitions 1 --priming 0
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all
```

The default is one unreported priming run and seven measured runs. The runner
prints the median `primary_value`, individual runs, and secondary CPU, memory,
render, trace, and verification metrics. Run it with `--help` for the
authoritative scenario, corpus, and option list.

Choose the scenario whose primary metric matches the problem:

| Scenario | Primary metric | Surface |
| --- | --- | --- |
| `large-paste` | `paste_complete_ms` | select/copy/tab-switch/paste/save workflow |
| `mixed-paste` | `paste_input_to_paint_ms` | shell-style mixed-language paste and first paint |
| `typing-medium`, `typing-large`, `typing-plain` | `typing_ms_per_char` | sustained editing with or without highlighting |
| `scroll-highlighted`, `scroll-plain` | `scroll_overrun_ms` | scheduled scroll input through redraw quiet |
| `open-small`, `open-large` | `open_to_first_frame_ms`, `open_to_quiet_ms` | process spawn through the first completed frame, and through redraw quiet |
| `search-large` | `search_reindex_ms` | find query reindexing |
| `multi-cursor-1k` | `viewport_paint_ms` | preparation and paint with 1,000 carets |
| `idle` | `idle_cpu_ms` | CPU, repaints, and RSS over two focused idle seconds |
| `latency-typing`, `latency-navigation` | `key_to_paint_ms_p50` | one key at a time: key press to first damaged frame, frames per key, per-frame cost |

Examples:

```sh
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario typing-large
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario typing-plain --corpus huge-plain-500k --typing-no-wrap
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario scroll-plain --corpus huge-plain-500k --position middle
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario mixed-paste --paste-target markdown --position end
```

`--position top|middle|end` helps distinguish local work from document-size
work. `--keep-temp` preserves the per-run trace files, whose `notify=<reason>`
and `startup_*_ms` lines attribute frames and startup phases.

The app records per-frame `frame_wall_ms` and `frame_cpu_ms` around render,
prepare, and paint. GPUI keeps presenting the last scene at the display rate
for one second after input, and some drivers report XDamage for a frame only
when the next one presents, so prefer app-side frame metrics and
`open_to_first_frame_ms` over `*_to_quiet` metrics when judging editor work. The opt-in `huge-rust-50k`, `huge-plain-500k`, and
`huge-mixed-concat-500k` corpora exercise the large-file envelope without
slowing the default `all` run. `mixed-paste` can replay an exact UTF-8 payload:

```sh
DISPLAY=:1 ./target/release/examples/bench_editor_x11 \
  --scenario mixed-paste --corpus-file /tmp/payload.txt \
  --paste-target markdown --position end
```

The benchmark needs a real X11 desktop with XTEST, XDamage, and `xclip`. Use the
same display, window size, build profile, corpus, scenario options, priming, and
repetition count for both sides of a comparison. If variance obscures the
result, increase repetitions before changing the metric or workload.

Do not commit machine-specific baseline numbers. Record the commit, host/CPU,
display, full command, primary values, and relevant diagnostics in the change
or pull request that uses them.

## Framework-neutral benchmarks

Criterion benchmarks isolate editor-model cost from GPUI, display, desktop
clipboard, and file I/O:

```sh
cargo bench -p lst-editor --bench editor_model
cargo bench -p lst-editor --bench vim_model
```

`editor_model` covers construction, typing, clipboard-shaped model effects,
navigation, find/replace, multi-cursor editing, and line edits. `vim_model`
covers motions, editing, registers, search, whole-document operations, and
oracle replay. Criterion workload names are defined in the benchmark sources;
use their command output rather than maintaining a second list here.

Save named Criterion baselines when useful:

```sh
CARGO_TARGET_DIR=/tmp/lst-editor-bench \
  cargo bench -p lst-editor --bench editor_model -- --save-baseline current
```

## Verification

A faster result is acceptable only if the relevant behavior remains green.
Run the focused fast tests while iterating, then the required checks in
[Testing](testing.md). Rendering, input, paste, search, scrolling, and startup
changes require the real-X11 behavior gate.
