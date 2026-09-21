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
| `scroll-highlighted`, `scroll-plain` | `scroll_frame_wall_ms_mean` | mean app-side frame time during a scheduled wheel scroll, with frames per second, the worst frame, and `scroll_overrun_ms` (input end through redraw quiet) as secondaries |
| `open-small`, `open-large` | `open_to_first_frame_ms`, `open_to_quiet_ms` | process spawn through the first completed frame, and through redraw quiet |
| `search-large` | `search_reindex_ms` | find query reindexing |
| `multi-cursor-1k` | `viewport_paint_ms` | first completed selection frame with 1,000 carets; not a later blink-hidden frame |
| `idle` | `idle_cpu_ms` | CPU, repaints, and RSS over two focused idle seconds |
| `latency-typing`, `latency-navigation`, `latency-edit-navigation` | `key_to_paint_ms_p50` | one key at a time: key press to first damaged frame, split into X delivery, app work through paint, and presentation; frames per key; per-frame cost |

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
prepare, and paint, and wall-clock stamps (`input_epoch_us` when a model
update starts, `frame_end_epoch_us` when a frame's paint ends) that the latency
scenarios use to report `key_delivery_ms_p50`, `key_to_frame_end_ms_p50`, and
`frame_end_to_damage_ms_p50`. `latency-edit-navigation` types a character
before each timed arrow key, the common case where revision-keyed caches have
just been invalidated.

`key_to_paint_ms` is the time from key injection to the first XDamage report
after the app's own `frame_end_epoch_us` for that key. Damage alone is not a
paint signal: GPUI re-presents the unchanged scene at the refresh rate for one
second after any input, and every present raises a damage report. Keys are
injected at evenly spread delays after the previous frame so the samples cover
every phase of GPUI's refresh timer rather than locking to one. Prefer
app-side frame metrics and `open_to_first_frame_ms` over `*_to_quiet` metrics
when judging editor work; the quiet metrics include that one-second
re-presentation.

`scroll_frames_per_second` counts app-rendered frames, not distinct refreshes
shown by the monitor. Redundant window notifications can increase this count
without improving presentation, and can skew mean frame cost toward cheap
unchanged frames. Inspect total CPU and frame counts alongside per-frame cost.

The measured `lst` must be the production build. The runner has no GPUI
dev-dependency, so building it alongside the app does not change the app's
feature set (Cargo unifies dev-dependency features across targets built in one
invocation). The opt-in `huge-rust-50k`, `huge-plain-500k`, and
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

`OPTIMIZATION_LOG.md` at the repository root records the measured changes,
the baselines they were compared against, and the framework and driver
behaviour that bounds what the app can improve.

## Patched GPUI

The workspace builds against `vendor/gpui`, the gpui 0.2.2 release with the
platform and rendering patches listed in `vendor/gpui/LST_PATCHES.md`. The
workspace also backports Blade's explicit Vulkan ray-tracing opt-in; GPUI
uses raster rendering only (see `vendor/blade-graphics/LST_PATCHES.md`). Each
patch records the measurement that motivated it; re-measure with the
`[patch.crates-io]` section of the root `Cargo.toml` removed to compare
against the unpatched release.

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
