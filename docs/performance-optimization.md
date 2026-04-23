# Performance Optimization Workflow

This document is for the active GPUI editor in `apps/lst-gpui`.

The goal is narrow:

- preserve behavior
- run one fixed benchmark scenario
- optimize one scalar metric that matches the user-visible problem
- use the other printed values as diagnostics

## GPUI Interaction Benchmark

The GPUI editor has a real-display X11 benchmark runner for editor interaction
latency. It launches the real `lst` binary, drives it through XTEST, watches
XDamage redraws, and verifies file contents for editing workflows.

Build the release app and runner:

```bash
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
```

Run every scenario:

```bash
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all
```

Run a short smoke pass:

```bash
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all --repetitions 1 --priming 0
```

Run one scenario while optimizing a specific path:

```bash
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario large-paste
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario typing-large
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario scroll-highlighted
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario search-large
```

The runner requires a real X11 desktop session with XTEST and XDamage. `Xvfb` is
not representative for this GPUI path on this host because GPUI surface creation
needs a real presentation backend.

The `large-paste` scenario also uses `xclip` to observe the X11 clipboard.

## Scenarios

Each scenario prints `primary_metric`, `primary_value`, per-run values, and
secondary diagnostics such as CPU time, damage events, peak RSS, and final file
size where relevant.

| Scenario | Primary metric | Completion condition |
| --- | --- | --- |
| `large-paste` | `paste_complete_ms` | Copies the large Rust corpus, pastes into a second file tab, then retries `Ctrl+S` until the target file exactly matches the corpus and stays stable. |
| `typing-medium` | `typing_ms_per_char` | Types a fixed lowercase payload into the generated medium Rust corpus, waits for redraw quiet, then verifies the saved file exactly matches the expected text. |
| `typing-large` | `typing_ms_per_char` | Same as `typing-medium`, using the generated large Rust corpus. |
| `scroll-highlighted` | `scroll_overrun_ms` | Scrolls down and back through the large Rust file on a fixed input schedule, then waits for redraw quiet. |
| `scroll-plain` | `scroll_overrun_ms` | Same scroll trace using the generated large plain-text corpus, so syntax highlighting is out of the path. |
| `open-large` | `open_to_quiet_ms` | Measures process spawn through benchmark window discovery and redraw quiet on the large Rust file. |
| `search-large` | `search_reindex_ms` | Opens find through `Ctrl+F`, clicks the visible find query input, types `fn `, waits for redraw quiet, and reads the completed in-app find reindex trace. |

The default runner contract is one priming run and seven measured repetitions.
Use `--repetitions <n>` and `--priming <n>` only when characterizing variance or
shortening a local smoke test.

The GPUI app writes internal benchmark trace values only when
`LST_BENCH_TRACE_FILE` is set by the runner. Normal editor runs do not create
trace files.

## Baselines

Do not keep stale baseline numbers in this document. Record comparison numbers
in the optimization branch or PR that uses them, with the commit SHA, display
session, scenario, repetitions, and priming count.

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
