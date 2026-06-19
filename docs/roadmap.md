# Roadmap

Keep `lst` minimal, fast, and production-grade. Prefer clear ownership and
observable behavior over feature volume.

## Current Architecture

- `lst-editor`: framework-neutral editor model, document primitives, text
  transactions, undo/redo snapshot history, observable snapshots, effects,
  language detection, line bookmarks, and Vim state
- `lst-gpui`: rendering, widgets, input adaptation, dialogs, clipboard, file
  I/O, tree-sitter syntax highlighting (`src/syntax`), DeepSeek text cleanup
  (`src/llm.rs`), benchmark wiring, and desktop integration

Editor-domain behavior should live in `lst-editor`; accepted product behavior
should be specified through the real GPUI app under the X11 harness whenever it
can be driven there. GPUI should adapt desktop events to editor contracts and
render observable state.

## Near-Term Priorities

Horizontal scrolling, find toggles, grapheme-aware motion, tab reordering,
multi-cursor creation gestures, and env-gated save options have landed; the
remaining near-term work is:

- Cursor blink and other small viewport polish
- Promote the `x11-tdd` specs that are already wired (line bookmarks, recently
  closed tab reopen) into the blocking `x11` profile, and add real-display
  coverage for syntax-highlighting colors
- User settings UI for the existing env-gated save options
  (trim-trailing-whitespace, ensure-final-newline) and a user-facing language
  picker (language is currently detection-only, with no override)
- Jump list and navigation history
- User-configurable keybindings
- Multi-cursor policy gaps: Vim-mode multi-cursor, join-line clusters, and
  find/replace over multiple selections

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
