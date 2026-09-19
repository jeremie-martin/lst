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
