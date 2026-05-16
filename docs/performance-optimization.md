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

## Vim Model Benchmark

`lst-editor` also has a non-X11 Criterion benchmark for Vim model behavior. It
does not launch GPUI, synthesize desktop input, wait for redraws, touch the
clipboard, or save files. It drives `EditorModel` through the same public Vim
input surface used by the model-level behavior tests, so it isolates Vim
state-machine and editor-model costs.

Run the Vim benchmark:

```bash
cargo bench -p lst-editor --bench vim_model
```

Run it with a named Criterion baseline:

```bash
CARGO_TARGET_DIR=/tmp/lst-vim-bench-target cargo bench -p lst-editor --bench vim_model -- --save-baseline current
```

To compare the current implementation with the rewrite workspace:

1. Create a patch containing only the Vim benchmark changes.
2. Apply that patch to clean temporary worktrees for
   `/home/holo/lst-vim-rewrite-codex` commits `2e1daaf` and `a604285`.
3. Run the same command in each worktree, using distinct baseline names such as
   `rewrite-baseline` and `rewrite-external-engine`.
4. Record the commit SHA, host/CPU, command, Criterion baseline name, and the
   primary timings for each workload.

The most useful workloads for identifying whole-document overhead are:

- `vim_motion/word`: repeated word motions across generated documents.
- `vim_edit/delete_word_undo`: repeated `dw` followed by `u`.
- `vim_edit/change_inner_word_escape_undo`: repeated `ciw`, insertion,
  escape, and undo.
- `vim_copy_paste/yank_paste_undo_lines`: repeated counted `yy`, `p`, and `u`
  with 16, 128, or 512 copied lines depending on document size.
- `vim_search/submit_query`: `/needle<Enter>` over the whole document.
- `vim_search/next_matches`: `/needle<Enter>` setup followed by repeated `n`.
- `vim_search/word_under_cursor`: repeated `*`, which reindexes the word under
  the cursor.
- `vim_whole_document/dgg_undo_from_end`: repeated `Gdggu`, deleting from the
  document end back to the top and restoring it through undo.
- `vim_oracle/replay_fixture`: replay of the generated Neovim oracle fixture.

Use the `small`, `medium`, and `large` variants to distinguish constant costs
from document-size scaling.

## Baselines

Do not keep stale baseline numbers in this document. Record comparison numbers
in the optimization branch or PR that uses them, with the commit SHA, display
session, scenario, repetitions, and priming count.

## Behavior Gate

The canonical behavior gate is the real-display X11 suite:

```bash
DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

Use the fast non-X11 suites for quick sanity while iterating:

```bash
cargo test
cargo test --all-features
```

Do not trust a performance change unless the X11 behavior gate stays green.
