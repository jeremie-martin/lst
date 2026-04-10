# Iced Text Editor Assessment

This note captures the local framework audit that was run after the large-paste
benchmark showed multi-second latency on a ~20k-line Rust file.

Scope:

- `iced = 0.14.0`
- local source audit only
- local benchmark and probe measurements only
- no internet search

## Question

Is `lst` using `iced` incorrectly, or is the built-in `text_editor` path itself
the limiting factor for very large pastes?

## Framework Usage Audit

The current `lst` integration matches the official `iced 0.14` examples:

- keep `text_editor::Content` in application state
- render it with `text_editor(&content)`
- route widget actions into `update`
- call `content.perform(action)` on edit actions

The official example in `/tmp/iced/examples/editor/src/main.rs` uses that exact
pattern.

This means the current `lst` usage is idiomatic `iced 0.14` usage for normal
interactive editing. The large-paste issue is not explained by an obvious misuse
of the widget API.

## What The Stack Actually Does

The relevant paths are:

1. `text_editor::Content::perform(Action)` in `iced_widget`
2. `Editor::perform(Action::Edit(Edit::Paste(...)))` in `iced_graphics`
3. `cosmic_text::Editor::insert_string(...)`

In other words, a normal paste is handled as an interactive insert into the
editor engine, not as a bulk whole-buffer replacement.

## Local Measurements

### Real benchmark

Using `bench_paste_x11` on the frozen 20k-line Rust corpus:

- `trace_wall_ms=2260.363`
- `paste_complete_ms=2029.025`
- `copy_clipboard_ms=9.625`
- `paste_update_total_ms=1365.340`
- `paste_perform_ms=1362.226`
- `paste_mark_changed_ms=2.923`

Interpretation:

- clipboard propagation is tiny
- app-owned post-edit bookkeeping is tiny
- most app-side time is inside the editor engine paste call itself

### UI ablations

The benchmark was then rerun with runtime-only ablations:

- baseline: `trace_wall_ms=2260.363`
- disable highlight: `trace_wall_ms=2076.438`
- disable gutter: `trace_wall_ms=2318.002`
- force no wrap: `trace_wall_ms=2301.896`
- disable highlight + gutter + wrap: `trace_wall_ms=2047.924`

Interpretation:

- syntax highlighting is a secondary cost
- gutter and wrap are not the dominant bottleneck on this workload
- even with obvious UI features stripped, the run stays around ~2.0s

### No-UI probe

A standalone local probe was run outside `lst` to compare the built-in
interactive paste path against bulk buffer creation:

- `iced_content_paste_ms=1265.097`
- `cosmic_editor_insert_ms=1283.680`
- `iced_content_with_text_ms=241.210`
- `cosmic_buffer_set_text_ms=245.017`

Interpretation:

- `iced` interactive paste cost closely matches raw `cosmic-text`
- the slowness survives outside the app, so it is not primarily caused by
  `lst` view code
- whole-buffer replacement is materially faster than interactive paste, but it
  is still far from "instantaneous"

## Conclusion

The current `lst` usage is idiomatic for `iced 0.14`, but the built-in
`text_editor` interactive paste path is not suitable for the performance target
of near-instant very large pastes.

Practical consequences:

- short-term: special-case large paste scenarios that can be modeled as whole
  buffer replacement
- medium-term: do not expect incremental tuning of gutters, wrapping, or local
  app bookkeeping to unlock orders-of-magnitude wins
- long-term: if near-instant huge pastes are a hard product requirement, the
  current `iced` `text_editor` stack is likely the wrong foundation unless a
  custom editor path replaces the built-in interactive paste behavior
