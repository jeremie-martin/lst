# Roadmap

Keep `lst` minimal, fast, and production-grade. Prefer clear ownership and
observable behavior over feature volume.

## Current Architecture

- `lst-editor`: framework-neutral editor model, document primitives, text
  transactions, undo/redo snapshot history, observable snapshots, effects, and
  Vim state
- `lst-gpui`: rendering, widgets, input adaptation, dialogs, clipboard, file
  I/O, benchmark wiring, and desktop integration

Editor-domain behavior should live in `lst-editor`; accepted product behavior
should be specified through the real GPUI app under the X11 harness whenever it
can be driven there. GPUI should adapt desktop events to editor contracts and
render observable state.

## Near-Term Priorities

- Horizontal scrolling when soft wrap is disabled
- Find toggles: case sensitivity, smart case, whole word, and regex
- Grapheme-aware motion in the main editor
- Cursor blink and other small viewport polish
- Trim-trailing-whitespace and ensure-final-newline save options
- Tab reordering and recently closed tab recovery
- Jump list and GPUI multi-cursor creation gestures
- User-configurable keybindings
- User-facing language picker for the existing model-level override

## Codebase Shape

- Keep model mutation behind explicit `EditorModel` APIs.
- Keep text mutation behind `EditRequest` / `TextChangeSet` transactions and
  `EditHistory` snapshot boundaries; focused request builders such as `text_input`,
  `multi_selection`, and `line_edit` should return transactions
  instead of mutating `EditorModel` directly. Multi-change batches must choose
  their primary change explicitly through `TextChangeSet::new`.
- Keep clipboard, filesystem, dialogs, focus, and rendering at the GPUI boundary.
- Split modules by real behavior responsibility, not by speculative layering.
- Avoid new traits or crates unless they remove production complexity.

## Quality Gates

- `DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only`
  is the canonical accepted-behavior gate.
- `cargo test` and `cargo test --all-features` are fast non-X11 sanity checks,
  not the source of truth for user-visible editor behavior.
- `cargo test -p lst-editor --features internal-invariants` covers optional
  private invariants when a refactor touches those internals.
- Performance work should use one benchmark scenario and one primary metric at a
  time, as described in `docs/performance-optimization.md`.
