# Optimization log

Concise record of measurement-driven performance work on the GPUI editor.
Numbers are from the reference host (i9-14900K, RTX 4090, 3840x2160@144Hz
physical display `:0`, scale factor 2.0) unless marked *nested* (off-screen
Xephyr, software presentation, only useful for relative app-side costs).
Commands: see `docs/performance.md`.

## Measurement additions

- `open-small`: process spawn to first completed frame (`open_to_first_frame_ms`),
  with app-side startup marks (`startup_app_init_ms`, `startup_model_ready_ms`,
  `startup_window_open_ms`, `startup_first_frame_ms`).
- `idle`: CPU, repaints, and RSS over two focused idle seconds.
- `latency-typing`, `latency-navigation`: one key at a time, key press to the
  first damaged frame (`key_to_paint_ms_p50/p95/max`), frames per key, and
  per-frame wall/CPU cost (`frame_wall_ms_*`, `frame_cpu_ms_*`).
- Trace labels `notify=<reason>` name every app-side redraw request so frame
  counts can be attributed without guessing.
- The summary no longer aborts when a secondary trace metric is absent from a
  run (for example a paste that takes the background syntax path).

## Baseline (before any change)

| Metric | Value |
| --- | --- |
| `open-small` open_to_first_frame_ms | 313 (app_init 203, model 54, first frame 26) |
| `latency-typing` key_to_paint_ms_p50 / p95 | 11.9 / 15.3, 4.05 frames per key |
| `latency-navigation` key_to_paint_ms_p50 / p95 | 9.3 / 12.3, 4.03 frames per key |
| `idle` idle_cpu_ms per 2 s | 20, 4 repaints (caret blink) |
| `typing-medium` typing_ms_per_char | 1.32 (syntax reparse 0.49 per char at line 0) |
| `scroll-plain` scroll_overrun_ms | 1065 (see note on presentation below) |

Where the time goes (perf, physical display):

- Every keystroke rendered four frames: model update, then a next-frame caret
  reveal, then the reveal's own notify, plus caret blink frames.
- GPUI keeps presenting the last scene at the display rate for one second
  after any input (`gpui::window` request-frame policy) and the NVIDIA driver
  reports XDamage for a frame only once the following frame presents. Both
  inflate `*_to_quiet` and `scroll_overrun` metrics on this host; they are
  framework/driver behaviour, not editor work.
- GPUI's `Application::new` loads the full system font database synchronously
  (12.8k font files here): ~200 ms of the 313 ms startup. Not fixable in the
  app; GPUI carries a `todo(linux) make font loading non-blocking`.
- Text shaping: cosmic-text 0.14 creates a rustybuzz shape plan per word, so a
  freshly visible code line costs ~0.2 ms to shape; scrolling is dominated by
  it (26% of app CPU nested).
- tree-sitter incremental reparse per keystroke: 0.16 ms mid-file, 0.5-2.5 ms
  when the edit sits at the top of a large file (error recovery).

## Changes

1. **Inline caret reveal** (`editor_view.rs`, `shell.rs`): the queued reveal is
   applied during `render` against the previous frame's geometry instead of a
   next-frame callback plus notify. Frames per key 4.05 -> 1.13 (the remainder
   is caret blink); *nested* key_to_paint p50 34 -> 9.8 ms.

2. **Cell-painted code lines** (`code_line.rs`): plain monospace ASCII
   segments are painted from a per-token glyph cache at column positions
   instead of being shaped by cosmic-text (one rustybuzz shape plan per word).
   Tokens are shaped once through `layout_line`, so ligatures inside
   punctuation runs are kept; a token whose advances are not uniform cells
   (proportional font, missing glyph) makes the segment fall back to GPUI
   shaping, as do tabs and non-ASCII text. Each line paints inside its own
   layer, matching GPUI's line painting, so primitives take one draw order
   instead of one bounds-tree insertion per glyph.
   Physical display: `latency-navigation` p50 9.3 -> 7.2 ms (p95 12.3 -> 10.1),
   mean frame 3.7 -> 2.4 ms; `scroll-plain` prepare max 10.1 -> 3.8 ms;
   `typing-medium` 1.32 -> 1.24 ms/char. The visual lane (baselines
   regenerated with the previous binary on this display) is pixel-identical
   for every scenario except the 3,400-character horizontally scrolled line,
   where glyphs previously drifted sub-pixel from the caret's column grid
   through accumulated float advances; they now sit exactly on it.

3. **First-frame warmup** (`main.rs`, `syntax/mod.rs`): the first frame spent
   13 ms compiling the tree-sitter highlight query (`Query::new` runs pattern
   analysis) and 6 ms resolving the editor font through GPUI's font database.
   Both are now done on a background thread started right after settings load,
   overlapping GPUI's ~57 ms window and Vulkan setup on the main thread.
   Startup phase marks (`startup_settings_loaded_ms`, `startup_app_new_ms`,
   `startup_inputs_ready_ms`, `startup_model_loaded_ms`,
   `startup_recent_loaded_ms`, `startup_views_ready_ms`) attribute the rest.
   First frame 25 -> 7 ms; `open-small` open_to_first_frame_ms 313 -> ~275.
   What remains is GPUI: ~200 ms loading the system font database and ~57 ms
   creating the window.

4. **Local row walk for vertical motion** (`lst-editor` `wrap.rs`, `motion.rs`,
   `lib.rs`): the model cached a full-document wrap layout keyed by revision,
   so the first arrow key after every edit rebuilt it (O(lines)), a second
   owner of the layout the app already patches incrementally. Display-row
   targets are now found by walking neighbouring lines, O(rows moved), and
   the model-side cache is gone. New `latency-edit-navigation` scenario
   (type, then time the arrow key), physical display, p50:
   17k-line Rust 15.1 -> 10.2 ms; 500k-line plain 87 -> 11.3 ms (max 102 -> 17).

## Latency anatomy

`latency-*` now split key-to-paint with app wall-clock stamps. Physical
display, `latency-navigation` middle of the 17k-line Rust corpus: X delivery
0.3 ms, app work through paint 2.6 ms, presentation 3.5 ms (7.2 ms p50).
Presentation grows to ~8.6 ms right after an edit because GPUI keeps
presenting the previous scene at the refresh timer for one second after
input, and that timer runs at 60 Hz here: `gpui::platform::linux::x11::client`
takes the first CRTC's mode (the secondary 1080p60 monitor) rather than the
monitor the window is on (3840x2160@144). Smooth scrolling is capped at 60 fps
for the same reason. Both are framework behaviour outside the app.

5. **No guessed-width wrap layout before the first paint** (`shell.rs`): the
   first render built a full wrap layout for an assumed viewport width, and
   the first paint immediately rebuilt it for the real width. The scroll
   extent now uses the logical line count for that one frame. 50k-line Rust
   first frame 18 -> 11.5 ms; 500k-line plain first frame 150 -> 77 ms.

6. **Byte scan for plain bracket structure** (`syntax/highlight.rs`): the
   plain structural snapshot (bracket pairs for files without a tree-sitter
   grammar, and the interim structure while a large file parses in the
   background) iterated every character through closures over the pair
   list. Pairs are ASCII, so a byte scan with a 256-entry class table finds
   them while continuation bytes are skipped; non-ASCII pair sets keep the
   character path, and a test checks both agree on multibyte text.
   500k-line plain open: view setup 74 -> 10 ms.

7. **Window title only when it changes** (`shell.rs`): every render set the
   window title, which GPUI implements as two X property changes with
   round trips. The title is now sent only when it differs from the last
   one sent.

## Results (physical display, 3 measured runs, medians)

| Scenario | Metric | Before | After |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 313 | 288 (GPUI init ~255 of it) |
| `open-large` 50k-line Rust | open_to_first_frame_ms | 334 | 297 |
| `latency-typing` | key_to_paint_ms p50 / p95 | 11.9 / 15.3 | 7.7 / 15.8 |
| `latency-navigation` | key_to_paint_ms p50 / p95 | 9.3 / 12.3 | 6.9 / 9.7 |
| `latency-edit-navigation` 17k lines | key_to_paint_ms p50 | 15.1 | 9.8 |
| `latency-edit-navigation` 500k lines | key_to_paint_ms p50 / max | 87 / 102 | 11.3 / 17 |
| frames per keystroke | count | 4.05 | 1.1 (rest is caret blink) |
| `scroll-plain` | viewport_prepare_ms max | 10.1 | 3.2 |
| `typing-medium` | typing_ms_per_char | 1.32 | 1.27 (tree-sitter reparse bound) |
| `idle` | idle_cpu_ms per 2 s | 20 | 30 (caret blink; unchanged) |
| first frame, 500k-line plain file | frame_wall_ms | 150 | 77 |

Per-key app work is now ~2.5 ms of a ~7 ms key-to-paint; the remainder is
X delivery (~0.3 ms) and presentation (~3.5 ms, up to ~8 ms right after an
edit) inside GPUI and the driver.

## Not done, and why

- GPUI loads the system font database (~200 ms here) and creates the
  window (~57 ms) before any app code runs; GPUI carries a
  `todo(linux) make font loading non-blocking`.
- GPUI's X11 refresh timer uses the first CRTC's mode (60 Hz on this host)
  rather than the window's monitor (144 Hz), capping smooth scrolling at
  60 fps; it also re-presents the last scene for one second after input.
  Both belong in GPUI (`platform/linux/x11/client.rs`).
- tree-sitter's incremental reparse per keystroke (0.16 ms mid-file, up to
  2.5 ms with error recovery at the top of a 660 KB file) is left
  synchronous: coalescing needs composed edit batches and moving it off the
  input path would paint one frame with stale highlights.
- Caret blink re-renders the whole window twice a second (~15 ms CPU/s).

## Session 2: measurement corrections and framework patches

The first session's numbers above were taken with two measurement faults,
found by profiling the production binary:

- **The benchmarked binary was not the production build.** Building the app
  and the runner in one `cargo build` unified the runner's
  `gpui` dev-dependency features (`test-support`, `leak-detection`) into the
  app. With `test-support`, GPUI draws inside `flush_effects`, i.e. right
  after input, whereas the production build only draws when the X11 refresh
  timer fires (`gpui::platform::linux::x11::client::start_refresh_loop`,
  60 Hz here). The dev-dependency is removed; nothing used it.
- **Key injection was phase-locked to that timer.** The runner injected each
  key a fixed delay after the previous paint, so every sample landed at the
  same timer phase and up to one period of latency was invisible. Keys are
  now injected at evenly spread delays. Damage reports also had to be paired
  with the app's frame stamp: GPUI presents the unchanged scene at the
  refresh rate for one second after any input, and every present raises
  XDamage (145 reports per keystroke measured with a probe), so "first damage
  after the key" often preceded the key's frame.

Corrected baseline, production binary at 119a528, physical display, medians
of 3 runs:

| Scenario | Metric | Corrected baseline |
| --- | --- | --- |
| `latency-navigation` | key_to_paint_ms p50 / p95 / max | 9.9 / 16.6 / 17.6 |
| `latency-typing` | key_to_paint_ms p50 / p95 / max | 10.4 / 17.3 / 17.8 |
| `open-small` | open_to_first_frame_ms | 297 |

Where the time goes (production binary): app work through paint ~2.5 ms
(navigation) / ~4.9 ms (typing); then a uniform 0-16.7 ms wait for the
60 Hz timer tick that draws; then ~1.5 ms to the present. Startup: font
database ~70-90 ms, Vulkan instance and device ~120 ms (NVIDIA), both
serialised before the window; window creation ~55 ms (surface and swapchain
~45, pipelines ~10); app setup and first frame ~35 ms. Memory: of ~300 MB
RSS on an empty file, ~200 MB is the NVIDIA driver (file-backed libraries
and `/dev/nvidiactl` mappings) and ~70 MB heap, mostly driver allocations.

### GPUI patches (`vendor/gpui`, see `vendor/gpui/LST_PATCHES.md`)

gpui 0.2.2 is the newest release and upstream `main` still has both
behaviours, so the fixes are carried as a vendored copy selected through
`[patch.crates-io]`; the exact diff is `vendor/gpui/lst.patch`.

8. **Draw right after X11 input.** Windows left dirty by an input batch are
   drawn and presented immediately instead of at the next refresh tick.
9. **Fastest monitor's refresh rate.** The refresh timer used the first
   CRTC's mode (the 60 Hz secondary display); it now uses the fastest active
   CRTC (144 Hz), which is also the smooth-scroll animation rate.
10. **Parallel system font scan.** The fontconfig directory walk parses font
    files on up to eight threads and avoids two `stat` calls per entry
    (~70 -> ~32 ms in isolation; mmap contention limits scaling past two
    threads).
11. **Vulkan context on a startup thread.** Instance and device creation
    (~120 ms) overlaps the font scan and X11 setup.

| Scenario | Metric | Corrected baseline | With patches |
| --- | --- | --- | --- |
| `latency-navigation` | key_to_paint_ms p50 / p95 / max | 9.9 / 16.6 / 17.6 | 6.1 / 8.4 / 12.2 |
| `latency-typing` | key_to_paint_ms p50 / p95 / max | 10.4 / 17.3 / 17.8 | 7.6 / 10.2 / 13.9 |
| `open-small` | open_to_first_frame_ms | 297 | ~240 (app_init 190 -> 135) |

Per key the remaining time is the app's own work (2.5 ms navigation, 4.9 ms
typing, of which tree-sitter's reparse is the largest piece) plus ~1.5-2.6 ms
from paint end to the damage report.

12. **Whitespace markers walk the slice** (`viewport.rs`): the marker scan
    did two rope character lookups per space to find runs; it now tracks
    neighbours while iterating the painted slice.

### Full suite with the patches (0e90458, physical display, 3 runs, medians)

The "previous" column is the first session's final table, which was measured
on the `test-support` binary with phase-locked injection, so the latency rows
are not comparable; the corrected production baseline is in the table above.

| Scenario | Metric | Previous | Now |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 310 | 227 |
| `open-large` 17k-line Rust | open_to_quiet_ms | 1452 | 1332 |
| `latency-typing` | key_to_paint_ms p50 | (10.4 corrected) | 7.9 |
| `latency-navigation` | key_to_paint_ms p50 | (9.9 corrected) | 6.4 |
| `latency-edit-navigation` | key_to_paint_ms p50 | 9.5 (biased) | 5.9 |
| `typing-medium` / `-large` / `-plain` | typing_ms_per_char | 1.17 / 1.35 / 0.79 | 1.17 / 1.40 / 0.72 |
| `idle` | idle_cpu_ms per 2 s | 30 | 20 |
| `multi-cursor-1k` | viewport_paint_ms | 5.8 | 5.8 |
| `search-large` | search_reindex_ms | 0.27 | 0.28 |
| `large-paste` | paste_complete_ms | 10.2 | 11.0 |
| `mixed-paste` | paste_input_to_paint_ms | 30 | 30-61 (see below) |
| `scroll-plain` / `-highlighted` | scroll_overrun_ms | 1111 / 1118 | 1070 / 1077 |

`mixed-paste` times one paste per run and swings between 30 and 61 ms for
either binary across runs (clipboard hand-off with `xclip`, and a
full-document plain bracket scan on paste); it needs more repetitions before
it can rank a change. The scroll overrun metrics remain bounded by GPUI's
one-second re-presentation after input.

### Per-frame preparation (6a4bfa6, and the marker scan commit after it)

A perf profile of smooth scrolling (468 frames in 3 s at 144 Hz) put the
app's prepare step at 0.62 ms per frame: rope line-to-char lookups per row,
identifier-window expansion for occurrence highlights even without a query,
cosmic-text shaping of every newly visible gutter number, a full-window
whitespace/control-character scan through rope slices (control characters
render by default), and cache trimming over four maps every frame.

13. Gutter numbers are painted from per-digit glyph cells (ten shaped
    glyphs shared by every line number) instead of shaping each label.
14. Line starts accumulate across the visible rows; display-line lengths are
    cached; per-line caches are trimmed only when the retained window moves.
15. Occurrence scan windows are built only when a query exists.
16. Painted windows carry their logical line, and the marker scan walks the
    cached display text (bytes on ASCII lines) instead of rope slices.

| Metric (scroll-plain, per frame) | Before | After |
| --- | --- | --- |
| viewport_prepare_ms | 0.62 | 0.24 |
| structure_decorations_ms | 0.21 | 0.05 |
| viewport_visible_highlights_ms | 0.04 | 0.00 |
| CPU per 3 s scroll run | 1710 ms | 1540 ms |

`latency-navigation` key_to_frame_end p50 2.37 -> 2.02 ms. Paint stays at
~0.68 ms per frame, all of it GPUI's per-glyph work (raster-bounds and atlas
hash lookups, primitive insertion); the element tree, layout, and scene
finish add ~1.2 ms per frame outside the viewport.

### Frame anatomy after the preparation work

Timing GPUI's draw phases directly (temporary instrumentation in the
vendored crate, steady-state caret-blink frames, 1,360x860 logical window
at scale 2):

| Phase | ms | Notes |
| --- | --- | --- |
| request_layout | 0.32 | app `render` plus GPUI element construction, 42 taffy nodes |
| taffy compute | 0.46 | 13 measured leaves, 83 measure calls per frame |
| prepaint | 0.27 | 0.23 of it the viewport prepare step |
| paint | 0.61 | 0.53 of it the viewport: ~2,000 `paint_glyph` calls |
| scene finish | 0.18 | GPUI sorts every glyph sprite by (order, tile) each frame |

Rendering without the tab strip and status bar (an experiment, not a
change) cut frame CPU from 1.98 to 1.04 ms and key-to-frame-end p50 from
2.0 to 1.1 ms: the ~35 chrome elements cost ~27 us each across GPUI's
layout, prepaint, paint, and hit-testing work. Marking the never-wrapping
chrome text `whitespace_nowrap` (so GPUI reuses its measured size across
taffy passes) made no measurable difference and was not kept. Reducing the
chrome to fewer, shallower elements is the remaining app-side lever for
frame cost; every option changes element structure and therefore needs the
pixel lane, so it is left for a dedicated change.

### Idle wakeups and the final numbers

The 144 Hz timer raised `idle` from 30 to 40-50 ms per 2 s: the extra
wakeups are kernel time (timerfd, epoll, socket poll), not app work. The
timer now runs at the monitor rate only for 1.2 s after an X11 event batch
(animations start from input, and GPUI re-presents for one second after it)
and at the previous 60 Hz cadence otherwise; an idle tick with nothing to
draw, run, or present returns before entering an app update. `idle` is back
to 20-30 ms per 2 s while scrolling still renders at 144 fps.

Environment note: after a monitor power cycle in the middle of the session
(both outputs are 4K afterwards), Vulkan device creation got slower for
every binary, so app_init measured ~185 ms for a build that measured
~135 ms before. The final table therefore compares the unpatched production
binary and the final build back to back in the same environment.

| Scenario | Metric | Unpatched (119a528, production build) | Final |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 334 | 244 |
| `latency-typing` | key_to_paint_ms p50 / p95 | 11.2 / 17.9 | 8.1 / 11.4 |
| `latency-navigation` | key_to_paint_ms p50 / p95 | 10.6 / 17.8 | 5.5 / 8.7 |
| `latency-edit-navigation` | key_to_paint_ms p50 / p95 | 8.4 / 17.4 | 6.0 / 8.4 |
| `idle` | idle_cpu_ms per 2 s | 30 | 20-30 |
| `typing-medium` / `-large` / `-plain` | typing_ms_per_char | 1.17 / 1.35 / 0.79 | 1.16 / 1.31 / 0.62 |
| `scroll-plain` | frames rendered in the 3 s scroll | 205 (60 fps) | 469 (144 fps), prepare 0.24 ms/frame |
| `multi-cursor-1k` | viewport_paint_ms | 5.7 | 5.8 |
| `search-large` | search_reindex_ms | 0.27 | 0.31 |
| `open-large` | open_to_quiet_ms | 1452 | 1369 |

Verification: `cargo test`, `cargo clippy --all-targets --all-features`,
the full nested X11 lane (265 tests; one off-screen-cursor test timed out
while the monitors were powered off and passes on rerun), and the physical
visual lane with baselines regenerated per scenario from the unpatched
binary: all ten scenarios are pixel-identical.

## Not done, and why (updated)

- GPUI's per-frame element work for the ~35 chrome elements (~0.95 ms) and
  its per-glyph paint path (~0.5 ms for a full 4K viewport) are the
  remaining frame cost; both need element restructuring or deeper GPUI
  changes and the pixel lane.
- Vulkan instance and device creation (~120 ms, NVIDIA driver, no layers)
  and surface/swapchain creation (~45 ms) bound startup; the former is
  overlapped with the font scan, the latter needs the window.
- tree-sitter's incremental reparse stays synchronous: 0.36 ms per
  keystroke mid-file, up to ~1 ms with error recovery at the top of a
  660 KB file.
- Memory is driver-dominated (~200 MB of the ~300 MB RSS on an empty file).
